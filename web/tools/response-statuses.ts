import { METHODS, STATUS_CODES } from "node:http";
import { resolve } from "node:path";
import openapiTS, { astToString, COMMENT_HEADER } from "openapi-typescript";
import ts from "typescript";

export function nameResponseStatuses(nodes: readonly ts.Node[]): ts.Node[] {
  const statuses = new Map<number, string>();
  const methods = new Set(METHODS);
  const propertyName = (member: ts.TypeElement): string | undefined =>
    ts.isPropertySignature(member) &&
    (ts.isIdentifier(member.name) || ts.isStringLiteral(member.name))
      ? member.name.text
      : undefined;
  const mapProperty = (
    member: ts.TypeElement,
    change: (node: ts.TypeLiteralNode) => ts.TypeLiteralNode,
  ): ts.TypeElement =>
    ts.isPropertySignature(member) && member.type && ts.isTypeLiteralNode(member.type)
      ? ts.factory.updatePropertySignature(
          member,
          member.modifiers,
          member.name,
          member.questionToken,
          change(member.type),
        )
      : member;
  const responseMap = (node: ts.TypeLiteralNode): ts.TypeLiteralNode =>
    ts.factory.updateTypeLiteralNode(
      node,
      ts.factory.createNodeArray(
        node.members.map((member): ts.TypeElement => {
          if (!ts.isPropertySignature(member) || !ts.isNumericLiteral(member.name)) return member;
          const status = Number(member.name.text);
          const description = STATUS_CODES[status];
          if (!description) {
            throw new Error(`Unsupported HTTP response status: ${member.name.text}`);
          }
          const name = `HTTP_STATUS_${description.toUpperCase().replace(/[^A-Z0-9]+/g, "_")}`;
          statuses.set(status, name);
          return ts.factory.updatePropertySignature(
            member,
            member.modifiers,
            ts.factory.createComputedPropertyName(ts.factory.createIdentifier(name)),
            member.questionToken,
            member.type,
          );
        }),
      ),
    );
  const operation = (node: ts.TypeLiteralNode): ts.TypeLiteralNode =>
    ts.factory.updateTypeLiteralNode(
      node,
      ts.factory.createNodeArray(
        node.members.map((member) =>
          propertyName(member) === "responses" ? mapProperty(member, responseMap) : member,
        ),
      ),
    );
  const transformed = nodes.map((node): ts.Node => {
    if (!ts.isInterfaceDeclaration(node) || !["paths", "operations"].includes(node.name.text))
      return node;
    const members = node.members.map((member) =>
      mapProperty(
        member,
        node.name.text === "operations"
          ? operation
          : (path) =>
              ts.factory.updateTypeLiteralNode(
                path,
                ts.factory.createNodeArray(
                  path.members.map((method) =>
                    methods.has(propertyName(method)?.toUpperCase() ?? "")
                      ? mapProperty(method, operation)
                      : method,
                  ),
                ),
              ),
      ),
    );
    return ts.factory.updateInterfaceDeclaration(
      node,
      node.modifiers,
      node.name,
      node.typeParameters,
      node.heritageClauses,
      ts.factory.createNodeArray(members),
    );
  });
  const declarations = [...statuses.entries()]
    .sort(([first], [second]) => first - second)
    .map(([status, name]) =>
      ts.factory.createVariableStatement(
        undefined,
        ts.factory.createVariableDeclarationList(
          [
            ts.factory.createVariableDeclaration(
              name,
              undefined,
              undefined,
              ts.factory.createNumericLiteral(status),
            ),
          ],
          ts.NodeFlags.Const,
        ),
      ),
    );
  return [...declarations, ...transformed];
}

export function assertSameContracts(original: readonly ts.Node[], named: readonly ts.Node[]): void {
  const typeNames = (nodes: readonly ts.Node[]): string[] =>
    nodes.flatMap((node) =>
      ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node) ? [node.name.text] : [],
    );
  const expected = typeNames(original);
  if (expected.length === 0 || JSON.stringify(expected) !== JSON.stringify(typeNames(named))) {
    throw new Error("Generated API type inventory is empty or changed");
  }
  const checks = expected
    .map((name) => `export type Contract_${name} = Assert<Equal<Original.${name}, Named.${name}>>;`)
    .join("\n");
  const operations = original.find(
    (node): node is ts.InterfaceDeclaration =>
      ts.isInterfaceDeclaration(node) && node.name.text === "operations",
  );
  const directKeys =
    operations?.members
      .map((member, index) => {
        if (
          !ts.isPropertySignature(member) ||
          !(ts.isIdentifier(member.name) || ts.isStringLiteral(member.name))
        ) {
          throw new Error("Unsupported operation declaration in the generated API");
        }
        const name = JSON.stringify(member.name.text);
        return `export type DirectKeys_${index} = Assert<Equal<keyof Original.operations[${name}]["responses"], keyof Named.operations[${name}]["responses"]>>;`;
      })
      .join("\n") ?? "";
  const methods = new Set(METHODS);
  const paths = original.find(
    (node): node is ts.InterfaceDeclaration =>
      ts.isInterfaceDeclaration(node) && node.name.text === "paths",
  );
  const pathKeys: string[] = [];
  for (const path of paths?.members ?? []) {
    if (
      !ts.isPropertySignature(path) ||
      !(ts.isIdentifier(path.name) || ts.isStringLiteral(path.name)) ||
      !path.type ||
      !ts.isTypeLiteralNode(path.type)
    )
      continue;
    for (const method of path.type.members) {
      if (
        !ts.isPropertySignature(method) ||
        !(ts.isIdentifier(method.name) || ts.isStringLiteral(method.name)) ||
        !methods.has(method.name.text.toUpperCase()) ||
        !method.type ||
        method.type.kind === ts.SyntaxKind.NeverKeyword
      )
        continue;
      const access = `[${JSON.stringify(path.name.text)}][${JSON.stringify(method.name.text)}]`;
      pathKeys.push(
        `export type PathKeys_${pathKeys.length} = Assert<Equal<keyof NonNullable<Original.paths${access}>["responses"], keyof NonNullable<Named.paths${access}>["responses"]>>;`,
      );
    }
  }
  const source = `
declare namespace Original { ${astToString([...original])} }
declare namespace Named { ${astToString([...named])} }
type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends
  (<T>() => T extends B ? 1 : 2) ? true : false;
type Assert<T extends true> = T;
${checks}
${directKeys}
${pathKeys.join("\n")}
`;
  const filename = resolve("response-contract.generated.ts");
  const options: ts.CompilerOptions = {
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    strict: true,
    noEmit: true,
    exactOptionalPropertyTypes: true,
    noUncheckedIndexedAccess: true,
    types: [],
  };
  const host = ts.createCompilerHost(options);
  const originalGetSourceFile = host.getSourceFile.bind(host);
  host.getSourceFile = (path, languageVersion, onError, create) =>
    path === filename
      ? ts.createSourceFile(path, source, languageVersion, true)
      : originalGetSourceFile(path, languageVersion, onError, create);
  const program = ts.createProgram([filename], options, host);
  const checked = program.getSourceFile(filename);
  if (!checked || checked.statements.length === 0) {
    throw new Error("No generated API contract was checked");
  }
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length > 0) {
    throw new Error(
      `Generated API types changed:\n${ts.formatDiagnostics(diagnostics, {
        getCurrentDirectory: () => process.cwd(),
        getCanonicalFileName: (path) => path,
        getNewLine: () => "\n",
      })}`,
    );
  }
}

export async function generateApiTypes(schema: URL): Promise<string> {
  const original = await openapiTS(schema);
  const named = nameResponseStatuses(original);
  assertSameContracts(original, named);
  return `${COMMENT_HEADER}${astToString(named)}`;
}
