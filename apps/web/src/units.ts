// Architectural feet-inches formatting (SPEC §1): nearest 1/16", fractions reduced, no feet under 12".
export function fmtFtIn(inches: number): string {
  if (!Number.isFinite(inches)) return '-';
  const neg = inches < 0;
  const total16 = Math.round(Math.abs(inches) * 16);
  if (total16 === 0) return '0"';
  const wholeIn = Math.floor(total16 / 16);
  let num = total16 % 16, den = 16;
  while (num && num % 2 === 0) { num /= 2; den /= 2; }
  const feet = Math.floor(wholeIn / 12), inch = wholeIn % 12;
  let body: string;
  const frac = num ? `${num}/${den}` : '';
  if (feet > 0) {
    const inchPart = inch || frac ? `${inch}${frac ? ' ' + frac : ''}` : '0';
    body = `${feet}'-${inchPart}"`;
  } else {
    body = inch ? `${inch}${frac ? ' ' + frac : ''}"` : `${frac}"`;
  }
  return (neg ? '-' : '') + body;
}

/** Scale factor -> "1-1/2\"=1'-0\"" style label when it is one of the standard architectural scales. */
export function scaleLabel(factor: number): string {
  const std: Record<number, string> = {
    4: '3"=1\'-0"', 8: '1 1/2"=1\'-0"', 12: '1"=1\'-0"', 16: '3/4"=1\'-0"', 24: '1/2"=1\'-0"', 32: '3/8"=1\'-0"', 48: '1/4"=1\'-0"',
  };
  return std[factor] ?? `1:${factor}`;
}
