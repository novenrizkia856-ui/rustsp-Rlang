import * as vscode from "vscode";

// ─── Effect System Knowledge Base ───────────────────────────────────────────
const EFFECTS_INFO: Record<string, { description: string; examples: string[] }> = {
  io: {
    description:
      "I/O operations — console, file, network, environment, and process I/O.",
    examples: [
      'println("...")',
      "File::open(...)",
      "TcpStream::connect(...)",
      "env::var(...)",
    ],
  },
  alloc: {
    description:
      "Heap memory allocation — Vec, Box, String, HashMap, and other heap constructors.",
    examples: [
      "Vec::new()",
      "Box::new(...)",
      'String::from("...")',
      "HashMap::new()",
      "vec![...]",
      'format!("...")',
    ],
  },
  panic: {
    description:
      "Operations that may panic at runtime — unwrap, expect, assert, and explicit panics.",
    examples: [
      ".unwrap()",
      '.expect("...")',
      "panic!(...)",
      "assert!(...)",
      "unreachable!()",
    ],
  },
  read: {
    description:
      "Read access to a parameter — indicates the function reads from the specified parameter's fields.",
    examples: ["effects(read x)", "x.field // read effect"],
  },
  write: {
    description:
      "Write/mutate access to a parameter — indicates the function mutates the specified parameter's fields. Treated as a linear resource (exclusive ownership).",
    examples: [
      "effects(write acc)",
      "acc.balance = acc.balance + amount // write effect",
    ],
  },
};

const KEYWORD_INFO: Record<string, string> = {
  outer:
    "RustS+ keyword: Explicitly modifies a variable from an outer scope. Prevents ambiguous shadowing (Logic-02, RSPL081).",
  mut: "Declares a mutable variable. Required for same-scope reassignment (Logic-06, RSPL071).",
  effects:
    "Declares the side effects a function may perform. Effects include: io, alloc, panic, read(x), write(x).",
  fn: "Declares a function. RustS+ syntax: fn name(param Type) effects(...) ReturnType { body }",
  struct:
    "Declares a struct. RustS+ syntax uses spaces instead of colons: struct Name { field Type }",
  enum: "Declares an enum. Pattern matching uses direct braces: Pattern { body } instead of Pattern => { body }",
  match:
    "Pattern matching expression. RustS+ match arms use { body } instead of => { body }.",
};

// ─── Completion Provider ────────────────────────────────────────────────────
class RustSPlusCompletionProvider
  implements vscode.CompletionItemProvider
{
  provideCompletionItems(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
    _context: vscode.CompletionContext
  ): vscode.CompletionItem[] {
    const lineText = document
      .lineAt(position)
      .text.substring(0, position.character);
    const items: vscode.CompletionItem[] = [];

    // Inside effects clause
    if (/effects\s*\([^)]*$/.test(lineText)) {
      for (const [name, info] of Object.entries(EFFECTS_INFO)) {
        const item = new vscode.CompletionItem(
          name,
          vscode.CompletionItemKind.Keyword
        );
        item.detail = `Effect: ${name}`;
        item.documentation = new vscode.MarkdownString(
          `**${name}** effect\n\n${info.description}\n\n**Detected in:**\n${info.examples.map((e) => `- \`${e}\``).join("\n")}`
        );
        item.sortText = `0_${name}`;
        items.push(item);
      }
      return items;
    }

    // RustS+ keywords
    const keywords = [
      { label: "fn", detail: "Function declaration", kind: vscode.CompletionItemKind.Keyword },
      { label: "struct", detail: "Struct definition", kind: vscode.CompletionItemKind.Keyword },
      { label: "enum", detail: "Enum definition", kind: vscode.CompletionItemKind.Keyword },
      { label: "impl", detail: "Implementation block", kind: vscode.CompletionItemKind.Keyword },
      { label: "trait", detail: "Trait definition", kind: vscode.CompletionItemKind.Keyword },
      { label: "match", detail: "Pattern matching", kind: vscode.CompletionItemKind.Keyword },
      { label: "if", detail: "Conditional expression", kind: vscode.CompletionItemKind.Keyword },
      { label: "else", detail: "Else branch", kind: vscode.CompletionItemKind.Keyword },
      { label: "while", detail: "While loop", kind: vscode.CompletionItemKind.Keyword },
      { label: "for", detail: "For loop", kind: vscode.CompletionItemKind.Keyword },
      { label: "loop", detail: "Infinite loop", kind: vscode.CompletionItemKind.Keyword },
      { label: "return", detail: "Return from function", kind: vscode.CompletionItemKind.Keyword },
      { label: "break", detail: "Break from loop", kind: vscode.CompletionItemKind.Keyword },
      { label: "continue", detail: "Continue loop", kind: vscode.CompletionItemKind.Keyword },
      { label: "mut", detail: "Mutable declaration", kind: vscode.CompletionItemKind.Keyword },
      { label: "outer", detail: "Outer scope mutation (RustS+)", kind: vscode.CompletionItemKind.Keyword },
      { label: "effects", detail: "Effect declaration clause (RustS+)", kind: vscode.CompletionItemKind.Keyword },
      { label: "mod", detail: "Module declaration", kind: vscode.CompletionItemKind.Keyword },
      { label: "pub", detail: "Public visibility", kind: vscode.CompletionItemKind.Keyword },
      { label: "use", detail: "Import declaration", kind: vscode.CompletionItemKind.Keyword },
      { label: "self", detail: "Self instance", kind: vscode.CompletionItemKind.Keyword },
      { label: "Self", detail: "Self type", kind: vscode.CompletionItemKind.Keyword },
      { label: "type", detail: "Type alias", kind: vscode.CompletionItemKind.Keyword },
      { label: "where", detail: "Where clause", kind: vscode.CompletionItemKind.Keyword },
      { label: "as", detail: "Type cast", kind: vscode.CompletionItemKind.Keyword },
    ];

    for (const kw of keywords) {
      const item = new vscode.CompletionItem(kw.label, kw.kind);
      item.detail = kw.detail;
      items.push(item);
    }

    // Built-in types
    const builtinTypes = [
      "i8", "i16", "i32", "i64", "i128",
      "u8", "u16", "u32", "u64", "u128",
      "f32", "f64", "bool", "char", "str",
      "usize", "isize",
      "String", "Vec", "Box", "Rc", "Arc",
      "HashMap", "HashSet", "BTreeMap", "BTreeSet",
      "Option", "Result",
    ];

    for (const t of builtinTypes) {
      const item = new vscode.CompletionItem(
        t,
        vscode.CompletionItemKind.TypeParameter
      );
      item.detail = "Type";
      items.push(item);
    }

    // RustS+ built-in functions
    const builtins = [
      { label: "println", detail: "Print line to stdout (RustS+ — no ! needed)", insertText: 'println("${1}"${2})' },
      { label: "print", detail: "Print to stdout (RustS+ — no ! needed)", insertText: 'print("${1}"${2})' },
      { label: "eprintln", detail: "Print line to stderr (RustS+ — no ! needed)", insertText: 'eprintln("${1}"${2})' },
      { label: "eprint", detail: "Print to stderr (RustS+ — no ! needed)", insertText: 'eprint("${1}"${2})' },
    ];

    for (const fn of builtins) {
      const item = new vscode.CompletionItem(
        fn.label,
        vscode.CompletionItemKind.Function
      );
      item.detail = fn.detail;
      item.insertText = new vscode.SnippetString(fn.insertText);
      items.push(item);
    }

    // Constants
    const consts = ["true", "false", "None", "Some", "Ok", "Err"];
    for (const c of consts) {
      const item = new vscode.CompletionItem(
        c,
        vscode.CompletionItemKind.Constant
      );
      item.detail = "Constant";
      items.push(item);
    }

    return items;
  }
}

// ─── Hover Provider ─────────────────────────────────────────────────────────
class RustSPlusHoverProvider implements vscode.HoverProvider {
  provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken
  ): vscode.Hover | null {
    const wordRange = document.getWordRangeAtPosition(position);
    if (!wordRange) return null;

    const word = document.getText(wordRange);
    const lineText = document.lineAt(position).text;

    // Check if inside an effects clause
    const beforeWord = lineText.substring(0, wordRange.start.character);
    const isInEffects = /effects\s*\([^)]*$/.test(beforeWord);

    if (isInEffects && word in EFFECTS_INFO) {
      const info = EFFECTS_INFO[word];
      const md = new vscode.MarkdownString();
      md.appendMarkdown(`### Effect: \`${word}\`\n\n`);
      md.appendMarkdown(`${info.description}\n\n`);
      md.appendMarkdown(`**Detected patterns:**\n`);
      for (const ex of info.examples) {
        md.appendMarkdown(`- \`${ex}\`\n`);
      }
      return new vscode.Hover(md, wordRange);
    }

    // Keyword info
    if (word in KEYWORD_INFO) {
      const md = new vscode.MarkdownString();
      md.appendMarkdown(`### \`${word}\`\n\n`);
      md.appendMarkdown(KEYWORD_INFO[word]);
      return new vscode.Hover(md, wordRange);
    }

    // Effects keyword standalone
    if (word === "effects") {
      const md = new vscode.MarkdownString();
      md.appendMarkdown("### `effects` clause\n\n");
      md.appendMarkdown(
        "Declares the side effects a function may perform.\n\n"
      );
      md.appendMarkdown("**Syntax:** `effects(io, alloc, panic, read x, write x)`\n\n");
      md.appendMarkdown(
        "Functions without `effects(...)` are **pure** — they cannot perform any side effects.\n\n"
      );
      md.appendMarkdown(
        "If a function calls another effectful function, it must propagate those effects."
      );
      return new vscode.Hover(md, wordRange);
    }

    return null;
  }
}

// ─── Document Symbol Provider ───────────────────────────────────────────────
class RustSPlusDocumentSymbolProvider
  implements vscode.DocumentSymbolProvider
{
  provideDocumentSymbols(
    document: vscode.TextDocument,
    _token: vscode.CancellationToken
  ): vscode.DocumentSymbol[] {
    const symbols: vscode.DocumentSymbol[] = [];

    for (let i = 0; i < document.lineCount; i++) {
      const line = document.lineAt(i);
      const text = line.text;

      // Match function definitions
      const fnMatch = text.match(
        /^\s*(?:pub\s+)?(?:async\s+)?(fn)\s+([a-zA-Z_][a-zA-Z0-9_]*)/
      );
      if (fnMatch) {
        const name = fnMatch[2];
        const effectsMatch = text.match(/effects\(([^)]*)\)/);
        const detail = effectsMatch ? `effects(${effectsMatch[1]})` : "pure";
        const symbol = new vscode.DocumentSymbol(
          name,
          detail,
          vscode.SymbolKind.Function,
          line.range,
          line.range
        );
        symbols.push(symbol);
        continue;
      }

      // Match struct definitions
      const structMatch = text.match(
        /^\s*(?:pub\s+)?(struct)\s+([A-Z][a-zA-Z0-9_]*)/
      );
      if (structMatch) {
        const symbol = new vscode.DocumentSymbol(
          structMatch[2],
          "struct",
          vscode.SymbolKind.Struct,
          line.range,
          line.range
        );
        symbols.push(symbol);
        continue;
      }

      // Match enum definitions
      const enumMatch = text.match(
        /^\s*(?:pub\s+)?(enum)\s+([A-Z][a-zA-Z0-9_]*)/
      );
      if (enumMatch) {
        const symbol = new vscode.DocumentSymbol(
          enumMatch[2],
          "enum",
          vscode.SymbolKind.Enum,
          line.range,
          line.range
        );
        symbols.push(symbol);
        continue;
      }

      // Match trait definitions
      const traitMatch = text.match(
        /^\s*(?:pub\s+)?(trait)\s+([A-Z][a-zA-Z0-9_]*)/
      );
      if (traitMatch) {
        const symbol = new vscode.DocumentSymbol(
          traitMatch[2],
          "trait",
          vscode.SymbolKind.Interface,
          line.range,
          line.range
        );
        symbols.push(symbol);
        continue;
      }

      // Match impl blocks
      const implMatch = text.match(
        /^\s*(impl)\s+(?:([A-Z][a-zA-Z0-9_]*)\s+for\s+)?([A-Z][a-zA-Z0-9_]*)/
      );
      if (implMatch) {
        const traitName = implMatch[2];
        const typeName = implMatch[3];
        const name = traitName
          ? `impl ${traitName} for ${typeName}`
          : `impl ${typeName}`;
        const symbol = new vscode.DocumentSymbol(
          name,
          "",
          vscode.SymbolKind.Class,
          line.range,
          line.range
        );
        symbols.push(symbol);
        continue;
      }

      // Match mod declarations
      const modMatch = text.match(
        /^\s*(?:pub\s+)?(mod)\s+([a-zA-Z_][a-zA-Z0-9_]*)/
      );
      if (modMatch) {
        const symbol = new vscode.DocumentSymbol(
          modMatch[2],
          "module",
          vscode.SymbolKind.Module,
          line.range,
          line.range
        );
        symbols.push(symbol);
      }
    }

    return symbols;
  }
}

// ─── Activation ─────────────────────────────────────────────────────────────
export function activate(context: vscode.ExtensionContext) {
  const selector: vscode.DocumentSelector = {
    language: "rustsplus",
    scheme: "file",
  };

  // Register completion provider
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      selector,
      new RustSPlusCompletionProvider(),
      "(" // Trigger after ( for effects clause
    )
  );

  // Register hover provider
  context.subscriptions.push(
    vscode.languages.registerHoverProvider(
      selector,
      new RustSPlusHoverProvider()
    )
  );

  // Register document symbol provider (Outline view & breadcrumbs)
  context.subscriptions.push(
    vscode.languages.registerDocumentSymbolProvider(
      selector,
      new RustSPlusDocumentSymbolProvider()
    )
  );

  console.log("RustS+ extension activated");
}

export function deactivate() {}
