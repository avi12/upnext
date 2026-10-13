// The Worker's bindings and settings, as `wrangler.toml` names them.

export interface Env {
  DB: D1Database;
  SNAPSHOTS: R2Bucket;
  // Cloudflare Access, which guards `/admin`: the team's address
  // (`https://<team>.cloudflareaccess.com`) and the Access application's
  // audience tag. Until both are set, the admin pages answer 404.
  ACCESS_TEAM_DOMAIN?: string;
  ACCESS_AUD?: string;
  // Tests only: Access's signing keys as a JSON Web Key Set, so the tests
  // can sign tokens without the network. It counts only when the team's
  // address is under `.example`; a real team's keys are always fetched from
  // its address.
  ACCESS_TEST_JWKS?: string;
  // Who runs this copy of the service and where to reach them, for the
  // terms page. Set in `wrangler.toml`'s `[vars]`.
  OPERATOR?: string;
  CONTACT_URL?: string;
}
