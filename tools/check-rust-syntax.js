#!/usr/bin/env node
// Syntax-check Rust sources without a Rust toolchain.
//
// The sandbox has no `cargo`/`rustc`, so edits to src/*.rs used to go in
// unverified. This parses files with tree-sitter's Rust grammar and reports
// every ERROR/MISSING node, which catches unbalanced braces, broken match arms,
// stray commas and the like. It is a parser, not a type checker: it proves the
// file is well-formed Rust, not that it compiles.
//
// Setup (once, anywhere outside the repo):
//   npm i web-tree-sitter tree-sitter-rust
// Run:
//   node tools/check-rust-syntax.js src/grass_wind.rs src/view_settings.rs
//   NODE_PATH=/tmp/rscheck/node_modules node tools/check-rust-syntax.js src/*.rs
//
// Known baseline noise (all pre-existing on main, all inside `raw` argument
// lists, so they are a grammar limitation rather than broken Rust):
//   display_gamma.rs (1), far_trees_gpu.rs (3), foliage_mips.rs (2),
//   gpu.rs (2), grass_mips.rs (3), material.rs (4)  ==  13 error nodes
// Any file outside that set that reports errors is a real regression; compare
// with the list above before blaming an edit.
'use strict';

const fs = require('fs');
const path = require('path');

function loadTreeSitter() {
  const candidates = [
    'web-tree-sitter',
    path.join(process.env.HOME || '', 'node_modules/web-tree-sitter'),
    '/tmp/rscheck/node_modules/web-tree-sitter',
  ];
  for (const c of candidates) {
    try {
      return require(c);
    } catch (_) {
      /* try next */
    }
  }
  console.error(
    'web-tree-sitter not found. Run:\n' +
      '  mkdir -p /tmp/rscheck && cd /tmp/rscheck && npm i web-tree-sitter tree-sitter-rust\n' +
      '  NODE_PATH=/tmp/rscheck/node_modules node tools/check-rust-syntax.js <files>'
  );
  process.exit(2);
}

function loadGrammar() {
  const candidates = [
    'tree-sitter-rust/tree-sitter-rust.wasm',
    '/tmp/rscheck/node_modules/tree-sitter-rust/tree-sitter-rust.wasm',
  ];
  for (const c of candidates) {
    try {
      return require.resolve(c);
    } catch (_) {
      /* try next */
    }
  }
  console.error('tree-sitter-rust grammar not found (npm i tree-sitter-rust).');
  process.exit(2);
}

(async () => {
  const { Parser, Language } = loadTreeSitter();
  await Parser.init();
  const language = await Language.load(loadGrammar());
  const parser = new Parser();
  parser.setLanguage(language);

  const files = process.argv.slice(2);
  if (!files.length) {
    console.error('usage: node tools/check-rust-syntax.js <file.rs>...');
    process.exit(2);
  }

  let bad = 0;
  for (const file of files) {
    const source = fs.readFileSync(file, 'utf8');
    const tree = parser.parse(source);
    const errors = [];
    (function walk(node) {
      if (node.type === 'ERROR' || node.isMissing) errors.push(node);
      for (let i = 0; i < node.childCount; i++) walk(node.child(i));
    })(tree.rootNode);
    if (!errors.length) {
      console.log(`${file}: OK`);
      continue;
    }
    bad++;
    console.log(`${file}: ${errors.length} error node(s)`);
    for (const error of errors.slice(0, 10)) {
      const text = source
        .slice(error.startIndex, Math.min(error.endIndex, error.startIndex + 70))
        .replace(/\n/g, '\\n');
      const where = `line ${error.startPosition.row + 1} col ${error.startPosition.column}`;
      console.log(`   ${error.type}${error.isMissing ? ' MISSING' : ''} at ${where}: '${text}'`);
    }
  }
  process.exit(bad ? 1 : 0);
})();
