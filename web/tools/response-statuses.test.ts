import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { METHODS, STATUS_CODES } from "node:http";
import test from "node:test";
import openapiTS, { astToString } from "openapi-typescript";
import ts from "typescript";
import { z } from "zod";
import { assertSameContracts, nameResponseStatuses } from "./response-statuses.ts";

function literalConstants(nodes: readonly ts.Node[]): Map<string, string> {
  const constants = new Map<string, string>();
  for (const node of nodes) {
    if (!ts.isVariableStatement(node)) continue;
    assert.equal(node.declarationList.flags & ts.NodeFlags.Const, ts.NodeFlags.Const);
    for (const declaration of node.declarationList.declarations) {
      assert.ok(ts.isIdentifier(declaration.name));
      assert.ok(declaration.initializer && ts.isNumericLiteral(declaration.initializer));
      assert.ok(!constants.has(declaration.name.text));
      constants.set(declaration.name.text, declaration.initializer.text);
    }
  }
  return constants;
}

function propertyName(name: ts.PropertyName, constants: ReadonlyMap<string, string>): string {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name)) {
    return name.text;
  }
  assert.ok(ts.isComputedPropertyName(name) && ts.isIdentifier(name.expression));
  const value = constants.get(name.expression.text);
  assert.ok(value);
  return value;
}

function operationResponseInventory(nodes: readonly ts.Node[]): string[] {
  const constants = literalConstants(nodes);
  const operations = nodes.find(
    (node): node is ts.InterfaceDeclaration =>
      ts.isInterfaceDeclaration(node) && node.name.text === "operations",
  );
  assert.ok(operations);
  const entries: string[] = [];
  for (const operation of operations.members) {
    assert.ok(ts.isPropertySignature(operation) && operation.type);
    assert.ok(ts.isTypeLiteralNode(operation.type));
    const responses = operation.type.members.find(
      (member) =>
        ts.isPropertySignature(member) && propertyName(member.name, constants) === "responses",
    );
    assert.ok(responses && ts.isPropertySignature(responses) && responses.type);
    assert.ok(ts.isTypeLiteralNode(responses.type));
    for (const response of responses.type.members) {
      assert.ok(ts.isPropertySignature(response));
      entries.push(
        `${propertyName(operation.name, constants)}/${propertyName(response.name, constants)}`,
      );
    }
  }
  return entries.sort();
}

function numericResponseKeys(nodes: readonly ts.Node[]): string[] {
  const keys: string[] = [];
  const visit = (node: ts.Node): void => {
    if (
      ts.isPropertySignature(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === "responses" &&
      node.type &&
      ts.isTypeLiteralNode(node.type)
    ) {
      for (const member of node.type.members) {
        if (ts.isPropertySignature(member) && ts.isNumericLiteral(member.name)) {
          keys.push(member.name.text);
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  for (const node of nodes) visit(node);
  return keys;
}

test("the actual producer's numeric response keys receive computed names", async () => {
  const original = await openapiTS(new URL("../openapi.json", import.meta.url));
  const keys = numericResponseKeys(original);
  assert.ok(keys.length > 0);
  const named = nameResponseStatuses(original);
  assert.deepEqual(numericResponseKeys(named), []);
  const input: unknown = JSON.parse(
    await readFile(new URL("../openapi.json", import.meta.url), "utf8"),
  );
  const schema = z
    .object({ paths: z.record(z.string(), z.record(z.string(), z.unknown())) })
    .parse(input);
  const operationSchema = z.object({
    operationId: z.string(),
    responses: z.record(z.string(), z.unknown()),
  });
  const methods = new Set(METHODS);
  const expected: string[] = [];
  for (const path of Object.values(schema.paths)) {
    for (const [method, value] of Object.entries(path)) {
      if (!methods.has(method.toUpperCase())) continue;
      const operation = operationSchema.parse(value);
      for (const code of Object.keys(operation.responses))
        expected.push(`${operation.operationId}/${code}`);
    }
  }
  assert.ok(expected.length > 0);
  assert.deepEqual(operationResponseInventory(original), expected.sort());
  assert.deepEqual(operationResponseInventory(named), expected.sort());
  assertSameContracts(original, named);
});

const fixture = JSON.stringify({
  openapi: "3.1.0",
  info: { title: "Response contract", version: "1.0.0" },
  paths: {
    "/fixture": {
      get: {
        operationId: "read",
        responses: {
          "200": {
            description: "Success",
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  properties: { value: { type: "string", readOnly: true } },
                },
              },
            },
          },
          "2XX": { description: "Success group" },
          default: { description: "Fallback" },
        },
      },
    },
  },
  components: {
    schemas: {
      DomainRecord: {
        type: "object",
        properties: { responses: { type: "object", properties: { "7": { type: "string" } } } },
      },
    },
  },
});

test("response-like domain data keeps its own numeric keys", async () => {
  const original = await openapiTS(fixture);
  const before = astToString(original);
  const named = nameResponseStatuses(original);
  assert.deepEqual(numericResponseKeys(named), ["7"]);
  assert.deepEqual(operationResponseInventory(named), operationResponseInventory(original));
  assertSameContracts(original, named);
  assert.equal(astToString(original), before);
});

test("inline operations preserve default and range responses and direct key types", async () => {
  const original = await openapiTS(fixture.replace('"operationId":"read",', ""));
  const named = nameResponseStatuses(original);
  assert.deepEqual(numericResponseKeys(named), ["7"]);
  assertSameContracts(original, named);
  const broken = ts.transform(
    [...original],
    [
      (context) => {
        const visit: ts.Visitor = (node) => {
          const changed =
            ts.isPropertySignature(node) &&
            ts.isNumericLiteral(node.name) &&
            node.name.text === "200"
              ? ts.factory.updatePropertySignature(
                  node,
                  node.modifiers,
                  ts.factory.createStringLiteral(node.name.text),
                  node.questionToken,
                  node.type,
                )
              : node;
          return ts.visitEachChild(changed, visit, context);
        };
        return (node): ts.Node => ts.visitNode(node, visit) ?? node;
      },
    ],
  );
  try {
    assert.throws(
      () => assertSameContracts(original, broken.transformed),
      /Generated API types changed/,
    );
  } finally {
    broken.dispose();
  }
});

test("every standard status has a distinct literal constant and the same response type", async () => {
  const codes = Object.keys(STATUS_CODES).sort();
  assert.ok(codes.length > 0);
  const original = await openapiTS(
    JSON.stringify({
      openapi: "3.1.0",
      info: { title: "Standard response statuses", version: "1.0.0" },
      paths: {
        "/fixture": {
          get: {
            operationId: "read",
            responses: Object.fromEntries(
              codes.map((code) => [code, { description: STATUS_CODES[Number(code)] }]),
            ),
          },
        },
      },
    }),
  );
  const named = nameResponseStatuses(original);
  const constants = literalConstants(named);
  assert.deepEqual([...constants.values()].sort(), codes);
  assert.deepEqual(operationResponseInventory(named), operationResponseInventory(original));
  assertSameContracts(original, named);
});

test("the compiler refuses changed keys, payload types, optionality, and readonly fields", async (context) => {
  const original = await openapiTS(fixture);
  const changes: ReadonlyArray<(node: ts.Node) => ts.Node> = [
    (node) =>
      ts.isPropertySignature(node) && ts.isNumericLiteral(node.name) && node.name.text === "200"
        ? ts.factory.updatePropertySignature(
            node,
            node.modifiers,
            ts.factory.createStringLiteral(node.name.text),
            node.questionToken,
            node.type,
          )
        : node,
    (node) =>
      ts.isPropertySignature(node) && ts.isNumericLiteral(node.name) && node.name.text === "200"
        ? ts.factory.updatePropertySignature(
            node,
            node.modifiers,
            ts.factory.createNumericLiteral("400"),
            node.questionToken,
            node.type,
          )
        : node,
    (node) =>
      ts.isPropertySignature(node) && ts.isIdentifier(node.name) && node.name.text === "value"
        ? ts.factory.updatePropertySignature(
            node,
            node.modifiers,
            node.name,
            node.questionToken,
            ts.factory.createKeywordTypeNode(ts.SyntaxKind.NumberKeyword),
          )
        : node,
    (node) =>
      ts.isPropertySignature(node) && ts.isIdentifier(node.name) && node.name.text === "value"
        ? ts.factory.updatePropertySignature(node, node.modifiers, node.name, undefined, node.type)
        : node,
    (node) =>
      ts.isPropertySignature(node) && ts.isIdentifier(node.name) && node.name.text === "value"
        ? ts.factory.updatePropertySignature(
            node,
            undefined,
            node.name,
            node.questionToken,
            node.type,
          )
        : node,
  ];
  const names = [
    "quoted response key",
    "different response code",
    "payload type",
    "required value",
    "mutable value",
  ];
  assert.equal(names.length, changes.length);
  for (const [index, change] of changes.entries()) {
    const name = names[index];
    assert.ok(name);
    await context.test(name, () => {
      let reached = false;
      const result = ts.transform(
        [...original],
        [
          (context) => {
            const visit: ts.Visitor = (node) => {
              const changed = change(node);
              reached ||= changed !== node;
              return ts.visitEachChild(changed, visit, context);
            };
            return (node): ts.Node => ts.visitNode(node, visit) ?? node;
          },
        ],
      );
      try {
        assert.ok(reached);
        assert.throws(
          () => assertSameContracts(original, result.transformed),
          /Generated API types changed/,
          astToString(result.transformed),
        );
      } finally {
        result.dispose();
      }
    });
  }
});

test("unknown numeric response statuses and empty type inventories are refused", async () => {
  const original = await openapiTS(fixture.replace('"200"', '"599"'));
  assert.throws(() => nameResponseStatuses(original), /Unsupported HTTP response status: 599/);
  assert.throws(() => assertSameContracts([], []), /empty or changed/);
  assert.throws(() => assertSameContracts(original, original.slice(1)), /empty or changed/);
});
