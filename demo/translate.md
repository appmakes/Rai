---
model: gpt-4o
temperature: 0.2
---

# Localize Xcode strings into 10 languages

Translate `demo/text.en.strings` into these locales and write one output file per locale:
- zh-Hans
- zh-Hant
- ja-JP
- ko-KR
- fr-FR
- de-DE
- es-ES
- it-IT
- pt-BR
- ru-RU

Requirements:
1. Read `demo/text.en.strings`.
2. Create `target/` if needed.
3. For each locale above, write `target/text.<locale>.strings`.
4. Always overwrite existing target files from scratch.
5. Keep each key unchanged (left side in quotes).
6. Translate only the value (right side in quotes).
7. Keep exact Xcode `.strings` formatting for every line: `"key" = "value";`
8. Preserve the same entry order and total count (40 lines).
9. Escape any internal double quotes in translated values.
10. After writing, read each generated file and confirm it has exactly 40 entries in valid `.strings` line format.
11. If any file is missing or invalid, fix it before finishing.

Tool and behavior constraints:
- Translate directly using your own language knowledge. Do not call external translation services.
- Do not use network tools for this task (`web_search`, `web_fetch`, `http_get`, `http_request`).
- Prefer only local file tools (`list_dir`, `file_read`, `file_write`, `file_edit`, `file_append`).
- Never fail only because you are unsure of phrasing; choose a natural translation and continue.

Completion rule:
- Only finish when all 10 output files exist under `target/` and pass your validation check.
