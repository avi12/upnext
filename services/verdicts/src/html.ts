// Plain HTML pages for the terms and the moderators, with no scripts, so a
// strict content security policy can forbid them outright.

export function escape(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string);
}

const STYLE = `body{font:16px/1.5 system-ui,sans-serif;max-width:60rem;margin:2rem auto;padding:0 1rem;color:#1d1d1f;background:#fff}
@media (prefers-color-scheme:dark){body{color:#e8e8ea;background:#16161a}a{color:#8ab4f8}}
table{border-collapse:collapse;width:100%}td,th{border-top:1px solid #8884;padding:.4rem;vertical-align:top;text-align:left}
code{font-size:.85em}.none{opacity:.7}blockquote{border-left:3px solid #8886;margin:0;padding-left:1rem}`;

export function page(title: string, body: string, opts: { admin?: boolean } = {}): Response {
  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escape(title)}</title><style>${STYLE}</style></head>
<body><h1>${escape(title)}</h1>
${body}
</body></html>
`;
  return new Response(html, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "content-security-policy": "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
      "x-content-type-options": "nosniff",
      // Not `no-referrer`: with it, browsers send `Origin: null` on a form
      // post, and the admin pages' forms would fail their origin check.
      "referrer-policy": "same-origin",
      "cache-control": opts.admin ? "no-store" : "public, max-age=3600",
    },
  });
}
