// `dotenv`, reduced to a no-op.
//
// `module/song_url_v1.js` calls `require('dotenv').config()` on every request
// to pick up ENABLE_PROXY / PROXY_URL. The embedded runtime has no `.env` file
// and no process environment to populate, so loading is a no-op — but it must
// *succeed*, because it sits on the hot path of the single most important
// endpoint. Treating it as an unavailable dependency would take out
// `/song/url/v1` entirely.

export const config = () => ({ parsed: {} });
export const parse = () => ({});
export const populate = () => {};

export default { config, parse, populate };
