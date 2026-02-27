---
model: gpt-4o
temperature: 0.5
---

# Code Review
I need you to act as a senior software engineer. Please review the following code file: `{{ filename }}`.

Your review should focus on:
1.  **Correctness**: Are there any bugs or logical errors?
2.  **Readability**: Is the code easy to understand? Are variable names descriptive?
3.  **Performance**: Are there any obvious performance bottlenecks?
4.  **Security**: Are there any potential security vulnerabilities?

Please provide your feedback in a structured Markdown format.

## security
I need you to act as a security expert. Please analyze the file `{{ filename }}` specifically for security vulnerabilities.

Look for:
- SQL injection risks (if applicable)
- Cross-site scripting (XSS) risks (if applicable)
- Insecure direct object references (IDOR)
- Hardcoded secrets or credentials
- Improper error handling

If you find any issues, please explain the impact and suggest a fix.

## refactor
I want to improve the quality of the code in `{{ filename }}`.

Please suggest a refactoring plan that:
- Improves code modularity.
- Reduces complexity (cyclomatic complexity).
- Enhances testability.
- Adheres to idiomatic patterns for the language used.

Do not rewrite the entire file unless necessary. Provide snippets of the proposed changes.

## docs
Please generate comprehensive documentation for the code in `{{ filename }}`.

Include:
- A high-level summary of what the file does.
- Detailed descriptions for each function and class.
- Usage examples if applicable.
- Explanation of any complex logic or algorithms.
