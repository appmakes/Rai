---
model: gpt-4o
temperature: 0.7
# List of arguments used in the main task
args:
  - filename
  - language
  - style?
---

# Code Generation
Generate a {{ language }} code snippet that reads {{ filename }} and prints its content.
If `{{ style }}` is empty, use a default coding style.

## test
---
# Arguments specific to this sub-task can be overridden here
args:
  - test_framework
---
Write a unit test using {{ test_framework }} for the code in {{ filename }}.

Run with positional args:

```bash
rai doc/template_task.md src/main.rs rust
```

Run with named flags:

```bash
rai doc/template_task.md --filename src/main.rs --language rust --style concise
```
