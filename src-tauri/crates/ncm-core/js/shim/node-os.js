// `node:os` for the embedded build.
//
// `tmpdir()` returns a virtual root — node-fs.js keys state off the basename,
// not the directory, so the value only has to be stable.
//
// `networkInterfaces()` is called by `util/client-sign.js` to derive a MAC
// address for the desktop client signature. Returning `{}` is the honest
// answer (the embedded runtime has no interface enumeration and does not want
// one — probing NICs from a music player is both a privacy surface and a
// per-call syscall); the caller already handles the empty case by falling back
// to a synthesized identifier.

export const tmpdir = () => "/ncm-state";
export const platform = () => "linux";
export const type = () => "Linux";
export const release = () => "0.0.0";
export const arch = () => "x64";
export const hostname = () => "localhost";
export const homedir = () => "/ncm-state";
export const networkInterfaces = () => ({});
export const cpus = () => [];
export const totalmem = () => 0;
export const freemem = () => 0;
export const EOL = "\n";

export default {
  tmpdir,
  platform,
  type,
  release,
  arch,
  hostname,
  homedir,
  networkInterfaces,
  cpus,
  totalmem,
  freemem,
  EOL,
};
