// An uncaught custom error with extra properties and a cause.
class HttpError extends Error {
  constructor(status, message, options) {
    super(message, options);
    this.name = 'HttpError';
    this.status = status;
    this.headers = { 'content-type': 'text/plain' };
  }
}
function request(path) {
  try {
    JSON.parse('{"broken": ');
  } catch (e) {
    throw new HttpError(502, `bad gateway for ${path}`, { cause: e });
  }
}
const err = (() => {
  try {
    request('/x');
  } catch (e) {
    return e;
  }
})();
console.log(err instanceof HttpError, err instanceof Error, err.name, err.status, err.cause.name);
console.log(String(err), Object.keys(err));
request('/api/users');
