/**
 * Build the editor content after appending AI-generated SQL to existing editor SQL.
 * Adds a blank-line separator when the editor is not empty.
 *
 * Trims trailing whitespace from the existing content so a buffer that already ends
 * with newlines does not accumulate extra blank lines between statements.
 */
export function buildAppendedEditorSql(currentEditorSql: string, newSql: string): string {
  const trimmed = currentEditorSql.replace(/\s+$/, "");
  return trimmed ? `${trimmed}\n\n${newSql}` : newSql;
}
