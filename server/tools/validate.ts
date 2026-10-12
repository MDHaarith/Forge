type Schema = {
  type?: string;
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  enum?: unknown[];
  minItems?: number;
  maxItems?: number;
};

/**
 * Checks a tool input against the subset of JSON Schema Forge tools use.
 * Tool inputs are streamed eagerly, so the API does not validate them: a truncated
 * or malformed input must be caught here before the tool runs.
 * Returns a list of problems; empty means valid.
 */
export function validate(value: unknown, schema: Schema, at = "input"): string[] {
  const problems: string[] = [];
  if (schema.enum && !schema.enum.includes(value)) {
    problems.push(`${at} must be one of ${schema.enum.map((v) => JSON.stringify(v)).join(", ")}`);
  }
  switch (schema.type) {
    case "object": {
      if (typeof value !== "object" || value === null || Array.isArray(value)) {
        return [`${at} must be an object`];
      }
      const record = value as Record<string, unknown>;
      for (const key of schema.required ?? []) {
        if (!(key in record)) problems.push(`${at}.${key} is required`);
      }
      for (const [key, sub] of Object.entries(schema.properties ?? {})) {
        if (key in record) problems.push(...validate(record[key], sub, `${at}.${key}`));
      }
      break;
    }
    case "array":
      if (!Array.isArray(value)) return [`${at} must be an array`];
      if (schema.minItems !== undefined && value.length < schema.minItems) {
        problems.push(`${at} needs at least ${schema.minItems} items`);
      }
      if (schema.maxItems !== undefined && value.length > schema.maxItems) {
        problems.push(`${at} allows at most ${schema.maxItems} items`);
      }
      if (schema.items) value.forEach((item, i) => problems.push(...validate(item, schema.items!, `${at}[${i}]`)));
      break;
    case "string":
      if (typeof value !== "string") problems.push(`${at} must be a string`);
      break;
    case "number":
      if (typeof value !== "number" || !Number.isFinite(value)) problems.push(`${at} must be a number`);
      break;
    case "integer":
      if (!Number.isInteger(value)) problems.push(`${at} must be an integer`);
      break;
    case "boolean":
      if (typeof value !== "boolean") problems.push(`${at} must be a boolean`);
      break;
  }
  return problems;
}
