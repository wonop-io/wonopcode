# Tools API

This document outlines the built-in Tools and how they can be used. The Tools provide functionality to read, write, search, patch, and manage tasks within this project.

## Tool usage
These Tools are used programmatically, often with the following pattern:

1. The user or system identifies a need to read or modify files, search, or manage tasks.
2. The user requests a Tool action with parameters.
3. The environment processes the request, performing the action.
4. A response is returned, which might show success, results, or an error.

Below are details of each Tool:

### todowrite
• Usage: Create, update, or finalize tasks in a collaborative to-do list.
• Parameters:
  - **todos**: An array of tasks, each with a unique id, content, status, and priority.
• Example:
```
to=functions.todowrite {
  "todos": [
    {
      "content": "Implement new API",
      "id": "api-task-1",
      "priority": "high",
      "status": "pending"
    }
  ]
}
```

### grep
• Usage: Search file contents using regular expressions.
• Parameters:
  - **pattern**: The regex pattern to match
  - **include**: (Optional) file pattern to limit searching
  - **path**: (Optional) directory from which the search is performed
• Example:
```
to=functions.grep {
  "pattern": "function\\s+myFunction",
  "include": "*.js"
}
```

### glob
• Usage: Locates files by a glob pattern.
• Parameters:
  - **pattern**: Glob pattern (e.g., "src/**/*.rs")
  - **path**: (Optional) search path
• Example:
```
to=functions.glob {
  "pattern": "**/*.rs",
  "path": "./src"
}
```

### write
• Usage: Write content to a file, overwriting if it exists.
• Parameters:
  - **filePath**: Absolute path to the file
  - **content**: The text content to write
• Example:
```
to=functions.write {
  "filePath": "/absolute/path/to/file.txt",
  "content": "This is a new file content"
}
```

### edit
• Usage: Performs an exact string replacement in a file.
• Parameters:
  - **filePath**: Absolute path to the file
  - **oldString**: The exact text to replace
  - **newString**: The text to replace the oldString with
  - **replaceAll**: (Optional) Boolean indicating if all occurrences should be replaced (default false)
• Example:
```
to=functions.edit {
  "filePath": "/absolute/path/code.rs",
  "oldString": "old_function_name",
  "newString": "new_function_name",
  "replaceAll": true
}
```

### multiedit
• Usage: Applies multiple replacements across multiple files in one atomic operation.
• Parameters:
  - **edits**: Array of edit objects, each with properties filePath, oldString, newString, and replaceAll.
• Example:
```
to=functions.multiedit {
  "edits": [
    {
      "filePath": "src/example.rs",
      "oldString": "let x = 1;",
      "newString": "let x = 2;",
      "replaceAll": false
    }
  ]
}
```

### lsp
• Usage: Perform Language Server Protocol operations, like definitions and references.
• Parameters:
  - **operation**: One of "definition", "references", "symbols", "hover"
  - **file**: The path to the file
  - **line**, **column**: For definition/references/hover, 0-based line and column
• Example:
```
to=functions.lsp {
  "operation": "hover",
  "file": "src/lib.rs",
  "line": 10,
  "column": 4
}
```

### patch
• Usage: Apply a patch describing additions, deletions, or updates across multiple files.
• Parameters:
  - **patch_text**: The full patch text.
• Example:
```
to=functions.patch {
  "patch_text": "*** Begin Patch\n*** Update File: src/lib.rs..."
}
```

### webfetch
• Usage: Fetch content from a URL.
• Parameters:
  - **url**: The URL to fetch
  - **format**: One of "text", "markdown", or "html"
  - **timeout**: (Optional) Timeout in seconds
• Example:
```
to=functions.webfetch {
  "url": "https://example.com/docs",
  "format": "text",
  "timeout": 30
}
```

### codesearch
• Usage: Search for code examples, documentation, or usage patterns.
• Parameters:
  - **query**: The search query
  - **tokens_num**: (Optional) number of tokens (max 50000)
• Example:
```
to=functions.codesearch {
  "query": "example code for HTTP requests in Rust",
  "tokens_num": 3000
}
```

### bash
• Usage: Execute a bash command in a subshell.
• Parameters:
  - **command**: The command string
  - **description**: A short explanation of what the command does
  - **timeout**: (Optional) Time limit in milliseconds
  - **run_in_background**: (Optional) If true, run asynchronously
  - **workdir**: (Optional) Directory to run the command in
• Example:
```
to=functions.bash {
  "command": "ls -la",
  "description": "List files in the current directory"
}
```

## Best Practices
• Validate parameters carefully and handle potential errors.
• Provide concise but descriptive step-by-step commands.
• Consider the security implications when using the Tools.

## Future Improvements
• Additional usage examples for more advanced scenarios.
• Cross-references to other docs (e.g., security model) regarding restricted Tools usage.

_End of Tools API Reference_
