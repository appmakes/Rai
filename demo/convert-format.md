---
model: gpt-4o
temperature: 0.2
args:
  - input
  - output
  - input_format?
  - output_format?
---

# Convert document format

Convert a document from one format to another.

Inputs:
- Source file path: `{{ input }}`
- Target file path: `{{ output }}`
- Input format override (optional): `{{ input_format }}`
- Output format override (optional): `{{ output_format }}`

Instructions:
1. Read the source document.
2. If input format override is empty, detect input format from the file extension/content.
3. If output format override is empty, detect target format from the target file extension.
4. Convert the content into the requested target format.
5. Create parent directories for the target file if needed.
6. Write the final converted output to the target path.
7. Confirm the target file exists and is non-empty before finishing.
