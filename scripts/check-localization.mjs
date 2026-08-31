#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import ts from 'typescript';

const ROOT = path.resolve(import.meta.dirname, '..');
const SOURCE_ROOT = path.join(ROOT, 'src');
const LOCALE_ROOT = path.join(SOURCE_ROOT, 'i18n', 'locales');
const LOCALES = ['en', 'de', 'es', 'pt', 'fr', 'zh-CN', 'ja', 'fi'];
const TRANSLATABLE_ATTRIBUTES = new Set(['aria-label', 'title', 'placeholder', 'label', 'alt', 'data-tooltip']);
for (const property of ['description', 'emptyLabel', 'confirmLabel', 'cancelLabel']) {
  TRANSLATABLE_ATTRIBUTES.add(property);
}
const TRANSLATABLE_PROPERTIES = new Set([
  'label', 'title', 'description', 'placeholder', 'ariaLabel', 'emptyLabel', 'confirmLabel', 'cancelLabel',
]);
const CORE_PROPERTY_LABELS = new Set([
  'Items', 'Media', 'Dimensions', 'Size', 'Type', 'Duration', 'Date Imported', 'Date Created', 'Date Modified',
]);
const USER_MESSAGE_CALL = /^(?:window\.(?:alert|confirm)|set[A-Za-z]*(?:Error|Message)|reportFailure|item)$/;
const ALLOWED_UNCHANGED_MESSAGES = new Set([
  'Apple', 'Bing Visual Search', 'Chromium FontFace', 'Discord', 'Dropbox', 'E-Hentai',
  'EU (QWERTZ / AZERTY / Nordic)', 'ExHentai', 'GitHub', 'Google Drive', 'namespace:tag',
  'Picto', 'Picto {value0}', 'SauceNAO', 'Sogou', 'TinEye', 'Twitter', 'URL: {value0}',
  'US (QWERTY)', 'X-BC', 'YouTube',
]);
const ALLOWED_UNCHANGED_BY_LOCALE = {
  pt: new Set(['{value0} sites']),
  fr: new Set(['Page {value0}', '{value0} sites']),
};

function productionSourceFiles(directory = SOURCE_ROOT) {
  const result = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'i18n' && entry.name !== 'generated') result.push(...productionSourceFiles(file));
    } else if (/\.tsx?$/.test(entry.name) && !/(?:\.test|\.spec)\.[^.]+$/.test(entry.name)) {
      result.push(file);
    }
  }
  return result;
}

function hasLanguage(value) {
  return /[A-Za-z\u00c0-\u024f\u3040-\u30ff\u3400-\u9fff]/.test(value);
}

function placeholders(value) {
  return [...value.matchAll(/\{([A-Za-z][A-Za-z0-9_]*)\}/g)].map((match) => match[1]).sort();
}

function requiresDistinctTranslation(message) {
  if (ALLOWED_UNCHANGED_MESSAGES.has(message)) return false;
  if (/^(?:https?:|[A-Z0-9-]{1,8}$)/.test(message)) return false;
  if (/^\{value\d+\}(?:[ ·×x–-]*\{value\d+\})*[ ·×x–-]*$/.test(message)) return false;
  return (message.match(/[A-Za-z]+/g) ?? []).length >= 2;
}

function location(sourceFile, node) {
  const line = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile)).line + 1;
  return `${path.relative(ROOT, sourceFile.fileName)}:${line}`;
}

function isCorePropertyIdentity(sourceFile, value) {
  return sourceFile.fileName.endsWith(path.join('features', 'inspector', 'Inspector.tsx'))
    && CORE_PROPERTY_LABELS.has(value);
}

function collectBranchStrings(node, output) {
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
    if (hasLanguage(node.text)) output.push(node);
    return;
  }
  if (ts.isConditionalExpression(node)) {
    collectBranchStrings(node.whenTrue, output);
    collectBranchStrings(node.whenFalse, output);
  } else if (ts.isParenthesizedExpression(node)) {
    collectBranchStrings(node.expression, output);
  } else if (ts.isTemplateExpression(node)) {
    const text = node.head.text + node.templateSpans.map((span) => span.literal.text).join('');
    if (hasLanguage(text)) output.push(node);
  }
}

function reportRawStrings(sourceFile, node, description) {
  const strings = [];
  collectBranchStrings(node, strings);
  for (const string of strings) {
    errors.push(`${location(sourceFile, string)} contains ${description}`);
  }
}

const usedMessages = new Set();
const errors = [];

for (const file of productionSourceFiles()) {
  const source = fs.readFileSync(file, 'utf8');
  const sourceFile = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );

  const visit = (node) => {
    if (ts.isCallExpression(node) && node.expression.getText(sourceFile) === 't') {
      const message = node.arguments[0];
      if (!message || !ts.isStringLiteral(message)) {
        errors.push(`${location(sourceFile, node)} localization messages must be string literals`);
      } else {
        usedMessages.add(message.text);
      }
    }

    if (ts.isCallExpression(node) && USER_MESSAGE_CALL.test(node.expression.getText(sourceFile))) {
      for (const argument of node.arguments) reportRawStrings(sourceFile, argument, 'a raw user-facing call message');
    }

    if (ts.isJsxText(node) && hasLanguage(node.getText(sourceFile))) {
      errors.push(`${location(sourceFile, node)} contains raw JSX text`);
    }

    if (ts.isJsxAttribute(node) && node.initializer && ts.isStringLiteral(node.initializer)
      && TRANSLATABLE_ATTRIBUTES.has(node.name.getText(sourceFile)) && hasLanguage(node.initializer.text)) {
      errors.push(`${location(sourceFile, node)} contains a raw ${node.name.getText(sourceFile)} attribute`);
    }

    if (ts.isPropertyAssignment(node)
      && TRANSLATABLE_PROPERTIES.has(node.name.getText(sourceFile).replace(/["']/g, ''))) {
      const identity = (ts.isStringLiteral(node.initializer) || ts.isNoSubstitutionTemplateLiteral(node.initializer))
        && isCorePropertyIdentity(sourceFile, node.initializer.text);
      if (!identity) reportRawStrings(sourceFile, node.initializer, 'a raw user-facing descriptor');
    }

    if (ts.isJsxExpression(node) && node.expression) {
      const attribute = ts.isJsxAttribute(node.parent) ? node.parent.name.getText(sourceFile) : null;
      if (!attribute || TRANSLATABLE_ATTRIBUTES.has(attribute)) {
        if (ts.isTemplateExpression(node.expression)
          && hasLanguage(node.expression.head.text + node.expression.templateSpans.map((span) => span.literal.text).join(''))) {
          errors.push(`${location(sourceFile, node.expression)} contains a raw user-facing template`);
        }
        const strings = [];
        collectBranchStrings(node.expression, strings);
        for (const string of strings) {
          if (!(ts.isCallExpression(string.parent) && string.parent.expression.getText(sourceFile) === 't')) {
            errors.push(`${location(sourceFile, string)} contains a raw conditional label`);
          }
        }
      }
    }

    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
}

const catalogs = Object.fromEntries(LOCALES.map((locale) => {
  const file = path.join(LOCALE_ROOT, `${locale}.json`);
  return [locale, JSON.parse(fs.readFileSync(file, 'utf8'))];
}));
const englishKeys = Object.keys(catalogs.en).sort();

for (const message of usedMessages) {
  if (!(message in catalogs.en)) errors.push(`English catalog is missing: ${JSON.stringify(message)}`);
}

for (const locale of LOCALES) {
  const catalog = catalogs[locale];
  const keys = Object.keys(catalog).sort();
  const missing = englishKeys.filter((key) => !(key in catalog));
  const extra = keys.filter((key) => !(key in catalogs.en));
  if (missing.length) errors.push(`${locale} catalog is missing ${missing.length} key(s): ${missing.slice(0, 5).join(', ')}`);
  if (extra.length) errors.push(`${locale} catalog has ${extra.length} extra key(s): ${extra.slice(0, 5).join(', ')}`);
  for (const key of englishKeys) {
    const value = catalog[key];
    if (typeof value !== 'string' || value.trim() === '') {
      errors.push(`${locale} catalog has an empty translation for ${JSON.stringify(key)}`);
      continue;
    }
    if (locale !== 'en'
      && value === catalogs.en[key]
      && !ALLOWED_UNCHANGED_BY_LOCALE[locale]?.has(key)
      && requiresDistinctTranslation(key)) {
      errors.push(`${locale} catalog leaves ${JSON.stringify(key)} untranslated`);
    }
    if (placeholders(value).join('|') !== placeholders(catalogs.en[key]).join('|')) {
      errors.push(`${locale} catalog has different placeholders for ${JSON.stringify(key)}`);
    }
  }
}

if (errors.length) {
  console.error(`Localization check failed with ${errors.length} issue(s):`);
  for (const error of errors.slice(0, 100)) console.error(`- ${error}`);
  if (errors.length > 100) console.error(`- …and ${errors.length - 100} more`);
  process.exit(1);
}

console.log(`Localization check passed (${englishKeys.length} messages across ${LOCALES.length} languages).`);
