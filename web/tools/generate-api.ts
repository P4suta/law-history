import { writeFile } from "node:fs/promises";
import { parseArgs } from "node:util";
import { generateApiTypes } from "./response-statuses.ts";

const { values } = parseArgs({ options: { output: { type: "string" } } });
if (!values.output) throw new Error("The generated API output path is required");
await writeFile(
  values.output,
  await generateApiTypes(new URL("../openapi.json", import.meta.url)),
  "utf8",
);
