// `/terms`: the service's terms and privacy page ("Terms and privacy
// for the service"), linked from UpNext's consent screen. It quotes the
// consent text from the contract, so the two can't drift apart. The operator
// reads it before launch; it isn't legal advice.

import { CONSENT_VERSION, CONTRACT, LICENCE } from "./contract";
import type { Env } from "./env";
import { page, escape as e } from "./html";
import * as L from "./limits";

export function terms(env: Env): Response {
  const operator = env.OPERATOR?.trim() || "the person or group who deployed this copy of the service";
  const contact = env.CONTACT_URL?.trim();
  const consent = CONTRACT.consent;
  const reasons = CONTRACT.report_reasons.map((r) => e(r.label.toLowerCase())).join(", ");
  const body = `
<p>This service collects the backlog verdicts UpNext's players choose to send, and publishes them once a day for anyone to download. It is run by ${e(operator)}${
    contact ? `, who can be reached at <a href="${e(contact)}">${e(contact)}</a>` : ""
  }. UpNext's source code, this service's included, is open source; this copy of the service is the operator's.</p>

<h2>What you agree to when you send a verdict</h2>
<p>UpNext shows you this before your first verdict leaves your computer (consent version ${CONSENT_VERSION}):</p>
<blockquote><p><b>${e(consent.title)}</b></p>${consent.paragraphs.map((p) => `<p>${e(p)}</p>`).join("")}</blockquote>
<p>Everything published is dedicated to the public domain under <a href="${e(CONTRACT.licence_url)}">CC0 1.0</a> (${e(LICENCE)}). Every verdict is stored with the licence and the consent version its sender accepted, and the service refuses a verdict without them.</p>

<h2>What is stored</h2>
<ul>
<li>Each verdict as sent: the game's Steam appid or GOG id, the answers, the optional text and stopping note, the vocabulary, licence and consent versions, and the UpNext version that sent it (never published).</li>
<li>The SHA-256 hash of the sending player's random key, never the key itself. It lets a player edit and take down their own verdicts and lets the service apply limits and blocks.</li>
<li>The day each verdict was sent and last changed, never a time of day.</li>
<li>Reports: the verdict, the reason and the day, with the reporting key's hash so one key's reports count once.</li>
<li>For rate limiting only: a count of the day's writes per client address (an IPv6 address by its /64), kept as a keyed hash under a random value made for that day. The counts and that day's value are deleted by the next daily run, so the address can't be recovered afterwards.</li>
<li>A log of moderators' actions, with the moderator's sign-in address and the day.</li>
</ul>

<h2>What is not stored or published</h2>
<ul>
<li>No IP address is stored by the service, beyond the day's hashed counts above; no store account, Steam ID, name, email address or library; no time zone or locale.</li>
<li>The published files carry months, never days, and each verdict's own random id, never the key's hash, so they don't say which verdicts came from the same person. Someone reading them might still guess at times, from a way of writing or an unusual mix of games. The operator can tell from the service's database, and doesn't publish it.</li>
</ul>

<h2>Where it runs</h2>
<p>The service runs on Cloudflare (Workers, D1 and R2). Every request to it, the address it comes from included, passes through Cloudflare, which handles it under its own <a href="https://www.cloudflare.com/privacypolicy/">privacy policy</a>; the verdicts and the snapshot are stored on Cloudflare's servers.</p>

<h2>Taking verdicts down</h2>
<p>UpNext's "Take down" withdraws a verdict and "Take down all my verdicts and forget my key" withdraws all of them. A withdrawn verdict is left out of the next daily snapshot, and the same daily run deletes its answers and text, keeping only its id, its game, the key's hash and its days, so an id is never used twice. Published files are kept for ${L.KEEP_DAYS} days and cached for at most a day, so a withdrawn verdict is gone from this service within a week. Copies people already downloaded can't be recalled, and a CC0 dedication is meant to be permanent, so what was published before may go on being shared.</p>
<p>A lost key means its verdicts can't be withdrawn by their writer, since nothing else links them to anyone. A verdict with personal information in it can still be reported, and a moderator can remove it.</p>

<h2>Reports and moderation</h2>
<p>Any published text can be reported as ${reasons}. A text reported by three different keys, each with a published verdict of its own, is held out of the snapshot until a moderator looks; moderators see every report. A game that suddenly gets far more verdicts than usual has its new ones held until a moderator releases them, or for seven days if nobody reports them. Moderators can hide a text, remove a verdict and block a key; a blocked key's verdicts leave the next snapshot and its new verdicts and reports are refused.</p>

<h2>Limits</h2>
<p>Per key: ${L.NEW_PER_DAY} new verdicts a day, ${L.REPLACED_PER_DAY} changed verdicts a day and ${L.REPORTS_PER_DAY} reports a day. Per client address (an IPv6 address by its /64): ${L.ADDRESS_WRITES_PER_DAY} verdicts and reports a day across all keys. Taking verdicts down is never limited.</p>

<h2>No support or warranty</h2>
<p>The service is provided as is, with no support and no warranty of any kind, and may stop at any time. If it does, UpNext keeps the last snapshot it downloaded, and anyone can run the service from UpNext's source code.</p>
`;
  return page("UpNext backlog verdicts: terms and privacy", body);
}
