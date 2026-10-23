// `node:http` / `node:https` for the embedded build.
//
// `util/request.js` constructs keep-alive Agents and hands them to axios. The
// Rust HTTP client owns connection pooling, so these are inert markers that
// exist only so the construction site does not throw. The axios shim ignores
// them entirely.

class Agent {
  constructor(options = {}) {
    this.options = options;
    this.keepAlive = Boolean(options.keepAlive);
  }
  destroy() {}
}

const unsupported = (name) => () => {
  throw new Error(`http.${name} is not available in the embedded NCM protocol runtime`);
};

export { Agent };
export const globalAgent = new Agent({ keepAlive: true });
export const request = unsupported("request");
export const get = unsupported("get");
export const createServer = unsupported("createServer");

export default { Agent, globalAgent, request, get, createServer };
