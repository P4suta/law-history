use crate::model::{Content, Element, LocatedElement};
use std::collections::BTreeMap;

const MAXIMUM_TREE_DEPTH: usize = 128;
const MAXIMUM_TREE_NODES: usize = 500_000;
const MAXIMUM_XML_BYTES: usize = 33_554_432;

pub fn base_text(root: &Element) -> String {
    fn visit(node: &Element, parent: law_domain::RubyContext, output: &mut String) {
        let role = match node.tag.as_str() {
            "Ruby" => law_domain::RubyRole::Ruby,
            "Rt" => law_domain::RubyRole::Reading,
            _ => law_domain::RubyRole::Content,
        };
        let context = law_domain::ruby_context(parent, role);
        if context == law_domain::RubyContext::Reading {
            return;
        }
        for content in &node.content {
            match content {
                Content::Text(text) => output.push_str(text),
                Content::Element(child) => visit(child, context, output),
            }
        }
    }
    let mut output = String::new();
    visit(root, law_domain::RubyContext::Content, &mut output);
    output
}

pub fn title_text(input: &str) -> Result<String, String> {
    if !input.contains('<') {
        return Ok(input.to_owned());
    }
    fn validate(node: &Element, unrestricted: bool) -> Result<(), String> {
        let unrestricted =
            unrestricted || matches!(node.tag.as_str(), "QuoteStruct" | "ArithFormula");
        if !unrestricted {
            for (name, value) in &node.attributes {
                if !law_domain::inline_title_attribute_allowed(
                    node.tag == "Line",
                    name == "Style",
                    matches!(
                        value.trim_matches([' ', '\t', '\r', '\n']),
                        "dotted" | "double" | "none" | "solid"
                    ),
                ) {
                    return Err(format!("Unsupported title attribute {}/{name}", node.tag));
                }
            }
        }
        for child in node.children() {
            let allowed = unrestricted
                || match node.tag.as_str() {
                    "LawTitle" => matches!(child.tag.as_str(), "Line" | "Ruby" | "Sup" | "Sub"),
                    "Line" => matches!(
                        child.tag.as_str(),
                        "QuoteStruct" | "ArithFormula" | "Ruby" | "Sup" | "Sub"
                    ),
                    "Ruby" => child.tag == "Rt",
                    _ => false,
                };
            if !allowed {
                return Err(format!(
                    "Unsupported title element {}/{}",
                    node.tag, child.tag
                ));
            }
            validate(child, unrestricted)?;
        }
        Ok(())
    }
    let root = parse_xml(&format!("<LawTitle>{input}</LawTitle>"))?;
    validate(&root, false)?;
    Ok(base_text(&root))
}

pub fn address_number(element: &Element) -> Option<&str> {
    element
        .attributes
        .get("Num")
        .map(String::as_str)
        .filter(|number| law_domain::number_can_address(number.as_bytes()))
}

pub fn validate_tree(root: &Element) -> Result<(), String> {
    fn visit(node: &Element, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if depth > MAXIMUM_TREE_DEPTH || *count > MAXIMUM_TREE_NODES {
            return Err("The element tree exceeds its depth or node bound".into());
        }
        if !law_domain::schema_name(node.tag.as_bytes()) {
            return Err("Element names must be ASCII XML schema names".into());
        }
        for child in node.children() {
            visit(child, depth + 1, count)?;
        }
        Ok(())
    }
    visit(root, 0, &mut 0)
}

pub fn parse_xml(input: &str) -> Result<Element, String> {
    if input.len() > MAXIMUM_XML_BYTES {
        return Err("XML exceeds the 32 MiB input limit".into());
    }
    let options = roxmltree::ParsingOptions {
        nodes_limit: MAXIMUM_TREE_NODES as u32,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(input, options)
        .map_err(|error| error.to_string())?;
    fn convert(node: roxmltree::Node<'_, '_>, depth: usize) -> Result<Element, String> {
        if depth > MAXIMUM_TREE_DEPTH {
            return Err("XML nesting exceeds 128 elements".into());
        }
        if node.tag_name().namespace().is_some()
            || node
                .attributes()
                .any(|attribute| attribute.namespace().is_some())
        {
            return Err("XML namespaces require an explicit supported schema mapping".into());
        }
        let mut content = Vec::new();
        for child in node.children() {
            if child.is_element() {
                content.push(Content::Element(convert(child, depth + 1)?));
            } else if child.is_text() {
                let text = child.text().unwrap_or_default();
                if !text.trim().is_empty()
                    || matches!(
                        node.tag_name().name(),
                        "Sentence"
                            | "LawTitle"
                            | "Line"
                            | "Ruby"
                            | "Rt"
                            | "Sup"
                            | "Sub"
                            | "QuoteStruct"
                    )
                {
                    content.push(Content::Text(text.to_owned()));
                }
            }
        }
        Ok(Element {
            tag: node.tag_name().name().into(),
            attributes: node
                .attributes()
                .map(|attribute| (attribute.name().into(), attribute.value().into()))
                .collect(),
            content,
        })
    }
    let root = convert(document.root_element(), 0)?;
    validate_tree(&root)?;
    Ok(root)
}

pub fn segment(element: &Element, ordinal: usize) -> String {
    if let Some(number) = address_number(element) {
        format!("{}[num={number}]", element.tag)
    } else {
        format!("{}[index={ordinal}]", element.tag)
    }
}

fn sibling_numbers(parent: &Element) -> BTreeMap<(&str, &str), usize> {
    let mut numbers = BTreeMap::new();
    for child in parent.children() {
        if let Some(number) = address_number(child) {
            *numbers.entry((child.tag.as_str(), number)).or_default() += 1;
        }
    }
    numbers
}

fn child_segment(
    element: &Element,
    ordinal: usize,
    numbers: &BTreeMap<(&str, &str), usize>,
) -> String {
    if address_number(element).is_some_and(|number| {
        numbers
            .get(&(element.tag.as_str(), number))
            .is_some_and(|count| *count > 1)
    }) {
        format!("{}[index={ordinal}]", element.tag)
    } else {
        segment(element, ordinal)
    }
}

pub fn flatten(root: &Element) -> BTreeMap<String, Element> {
    fn visit(node: &Element, path: String, output: &mut BTreeMap<String, Element>) {
        output.insert(path.clone(), node.clone());
        let mut counts = BTreeMap::<String, usize>::new();
        let numbers = sibling_numbers(node);
        for child in node.children() {
            let count = counts.entry(child.tag.clone()).or_default();
            *count += 1;
            visit(
                child,
                format!("{path}/{}", child_segment(child, *count, &numbers)),
                output,
            );
        }
    }
    let mut result = BTreeMap::new();
    visit(root, "/Law".into(), &mut result);
    result
}

pub fn index(root: &Element) -> BTreeMap<String, &Element> {
    fn visit<'a>(node: &'a Element, path: String, output: &mut BTreeMap<String, &'a Element>) {
        output.insert(path.clone(), node);
        let mut counts = BTreeMap::<String, usize>::new();
        let numbers = sibling_numbers(node);
        for child in node.children() {
            let count = counts.entry(child.tag.clone()).or_default();
            *count += 1;
            visit(
                child,
                format!("{path}/{}", child_segment(child, *count, &numbers)),
                output,
            );
        }
    }
    let mut output = BTreeMap::new();
    visit(root, "/Law".into(), &mut output);
    output
}

pub fn is_main_article(path: &str, index: &BTreeMap<String, &Element>) -> bool {
    use law_domain::{ProvisionContext, ProvisionRole, provision_context};
    let mut context = ProvisionContext::Start;
    for end in path
        .match_indices('/')
        .map(|(offset, _)| offset)
        .filter(|offset| *offset != 0)
        .chain(std::iter::once(path.len()))
    {
        let Some(node) = index.get(&path[..end]) else {
            return false;
        };
        let role = match node.tag.as_str() {
            "Law" => ProvisionRole::Law,
            "LawBody" => ProvisionRole::Body,
            "MainProvision" => ProvisionRole::Main,
            "Part" | "Chapter" | "Section" | "Subsection" | "Division" => {
                ProvisionRole::Subdivision
            }
            "Article" => ProvisionRole::Article,
            _ => ProvisionRole::Other,
        };
        context = provision_context(context, role);
    }
    context == ProvisionContext::Article
}

pub fn child_paths(parent: &Element, path: &str) -> Vec<String> {
    let numbers = sibling_numbers(parent);
    let mut counts = BTreeMap::<&str, usize>::new();
    parent
        .children()
        .map(|child| {
            let ordinal = counts.entry(&child.tag).or_default();
            *ordinal += 1;
            format!("{path}/{}", child_segment(child, *ordinal, &numbers))
        })
        .collect()
}

pub fn provisions(root: &Element) -> Vec<LocatedElement> {
    flatten(root)
        .into_iter()
        .filter(|(_, element)| {
            matches!(
                element.tag.as_str(),
                "Article"
                    | "Paragraph"
                    | "Item"
                    | "SupplProvision"
                    | "AppdxTable"
                    | "AppdxStyle"
                    | "AppdxFormat"
                    | "TableStruct"
                    | "Sentence"
            )
        })
        .map(|(path, element)| LocatedElement { path, element })
        .collect()
}

pub fn replace(
    root: &Element,
    path: &str,
    replacement: Option<&Element>,
) -> Result<Element, String> {
    fn walk(
        node: &Element,
        current: &str,
        path: &str,
        replacement: Option<&Element>,
        found: &mut bool,
    ) -> Element {
        let mut output = node.clone();
        let mut counts = BTreeMap::<String, usize>::new();
        let numbers = sibling_numbers(node);
        output.content = node
            .content
            .iter()
            .filter_map(|part| match part {
                Content::Text(text) => Some(Content::Text(text.clone())),
                Content::Element(child) => {
                    let count = counts.entry(child.tag.clone()).or_default();
                    *count += 1;
                    let child_path =
                        format!("{current}/{}", child_segment(child, *count, &numbers));
                    if child_path == path {
                        *found = true;
                        replacement.cloned().map(Content::Element)
                    } else {
                        Some(Content::Element(walk(
                            child,
                            &child_path,
                            path,
                            replacement,
                            found,
                        )))
                    }
                }
            })
            .collect();
        output
    }
    if path == "/Law" {
        return replacement
            .cloned()
            .ok_or_else(|| "The Law root cannot be deleted".into());
    }
    let mut found = false;
    let result = walk(root, "/Law", path, replacement, &mut found);
    if found {
        Ok(result)
    } else {
        Err(format!("No element at {path}"))
    }
}

pub fn append(
    root: &Element,
    parent: &str,
    element: &Element,
    before: Option<&str>,
) -> Result<Element, String> {
    let mut target = flatten(root)
        .remove(parent)
        .ok_or_else(|| format!("No parent at {parent}"))?;
    if let Some(path) = before {
        let mut counts = BTreeMap::<String, usize>::new();
        let numbers = sibling_numbers(&target);
        let mut offset = None;
        for (index, content) in target.content.iter().enumerate() {
            if let Content::Element(child) = content {
                let count = counts.entry(child.tag.clone()).or_default();
                *count += 1;
                if format!("{parent}/{}", child_segment(child, *count, &numbers)) == path {
                    offset = Some(index);
                }
            }
        }
        target.content.insert(
            offset.ok_or_else(|| format!("No sibling at {path}"))?,
            Content::Element(element.clone()),
        );
    } else {
        target.content.push(Content::Element(element.clone()));
    }
    replace(root, parent, Some(&target))
}

pub fn move_element(
    root: &Element,
    path: &str,
    parent: &str,
    before: Option<&str>,
) -> Result<Element, String> {
    if path == "/Law" {
        return Err("The Law root cannot be moved".into());
    }
    struct Move<'a> {
        path: &'a str,
        parent: &'a str,
        before: Option<&'a str>,
        element: Element,
        inserted: bool,
    }
    fn walk(node: &Element, current: &str, movement: &mut Move<'_>) -> Element {
        let mut output = node.clone();
        output.content.clear();
        let mut counts = BTreeMap::<String, usize>::new();
        let numbers = sibling_numbers(node);
        for part in &node.content {
            match part {
                Content::Text(text) => output.content.push(Content::Text(text.clone())),
                Content::Element(child) => {
                    let count = counts.entry(child.tag.clone()).or_default();
                    *count += 1;
                    let child_path =
                        format!("{current}/{}", child_segment(child, *count, &numbers));
                    if current == movement.parent && movement.before == Some(child_path.as_str()) {
                        output
                            .content
                            .push(Content::Element(movement.element.clone()));
                        movement.inserted = true;
                    }
                    if child_path != movement.path {
                        output
                            .content
                            .push(Content::Element(walk(child, &child_path, movement)));
                    }
                }
            }
        }
        if current == movement.parent && movement.before.is_none() {
            output
                .content
                .push(Content::Element(movement.element.clone()));
            movement.inserted = true;
        }
        output
    }
    let element = index(root)
        .get(path)
        .copied()
        .cloned()
        .ok_or_else(|| format!("No source at {path}"))?;
    let mut movement = Move {
        path,
        parent,
        before,
        element,
        inserted: false,
    };
    let result = walk(root, "/Law", &mut movement);
    if movement.inserted {
        Ok(result)
    } else {
        Err(format!(
            "No admissible destination at {parent} before {before:?}"
        ))
    }
}

pub fn render_text(root: &Element) -> String {
    fn visit(element: &Element, output: &mut String) {
        for part in &element.content {
            match part {
                Content::Text(text) => output.push_str(text),
                Content::Element(child) => visit(child, output),
            }
        }
        if matches!(
            element.tag.as_str(),
            "Sentence" | "LawTitle" | "ArticleTitle" | "ParagraphNum" | "ItemTitle" | "TableRow"
        ) {
            output.push('\n');
        }
    }
    let mut output = String::new();
    visit(root, &mut output);
    output
}
