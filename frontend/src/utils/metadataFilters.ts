import { MetadataFilterNode } from '../api/types';

export type FilterMode = 'and' | 'or' | 'not';

export interface FilterRow {
  key: string;
  value: string;
  mode: FilterMode;
}

/** Try to parse a string as a JSON primitive (number, boolean, null), falling back to string. */
function parseFilterValue(raw: string): string | number | boolean | null {
  const trimmed = raw.trim();
  if (trimmed === 'true') return true;
  if (trimmed === 'false') return false;
  if (trimmed === 'null') return null;
  const num = Number(trimmed);
  if (trimmed !== '' && !isNaN(num) && isFinite(num)) return num;
  return trimmed;
}

export function buildFilterNode(rows: FilterRow[]): MetadataFilterNode | undefined {
  const validRows = rows.filter(r => r.key.trim() && r.value.trim());
  if (validRows.length === 0) return undefined;

  const nodes: MetadataFilterNode[] = validRows.map(r => {
    const leaf: MetadataFilterNode = { key: r.key.trim(), value: parseFilterValue(r.value) };
    if (r.mode === 'not') return { not: leaf };
    return leaf;
  });

  // Group: AND conditions together, OR conditions together
  const andNodes = nodes.filter((_, i) => validRows[i].mode !== 'or');
  const orNodes = nodes.filter((_, i) => validRows[i].mode === 'or');

  if (orNodes.length > 0 && andNodes.length > 0) {
    const andPart: MetadataFilterNode = andNodes.length === 1 ? andNodes[0] : { and: andNodes };
    return { or: [andPart, ...orNodes] };
  }
  if (orNodes.length > 0) {
    return orNodes.length === 1 ? orNodes[0] : { or: orNodes };
  }
  return andNodes.length === 1 ? andNodes[0] : { and: andNodes };
}
