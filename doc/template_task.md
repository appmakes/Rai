---
model: gpt-4o
temperature: 0.7
# List of arguments used in the main task
args:
  - filename
  - language
---

# Code Generation
Generate a {{ language }} code snippet that reads {{ filename }} and prints its content.

## test
---
# Arguments specific to this sub-task can be overridden here
args:
  - test_framework
---
Write a unit test using {{ test_framework }} for the code in {{ filename }}.
