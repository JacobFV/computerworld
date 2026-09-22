"""http: status codes and methods."""
from enum import IntEnum

__all__ = ['HTTPStatus', 'HTTPMethod']
__path__ = []

_STATUS = [
    (100, 'CONTINUE', 'Continue', 'Request received, please continue'),
    (101, 'SWITCHING_PROTOCOLS', 'Switching Protocols',
     'Switching to new protocol; obey Upgrade header'),
    (102, 'PROCESSING', 'Processing', ''),
    (103, 'EARLY_HINTS', 'Early Hints', ''),
    (200, 'OK', 'OK', 'Request fulfilled, document follows'),
    (201, 'CREATED', 'Created', 'Document created, URL follows'),
    (202, 'ACCEPTED', 'Accepted', 'Request accepted, processing continues off-line'),
    (203, 'NON_AUTHORITATIVE_INFORMATION', 'Non-Authoritative Information',
     'Request fulfilled from cache'),
    (204, 'NO_CONTENT', 'No Content', 'Request fulfilled, nothing follows'),
    (205, 'RESET_CONTENT', 'Reset Content', 'Clear input form for further input'),
    (206, 'PARTIAL_CONTENT', 'Partial Content', 'Partial content follows'),
    (300, 'MULTIPLE_CHOICES', 'Multiple Choices', 'Object has several resources -- see URI list'),
    (301, 'MOVED_PERMANENTLY', 'Moved Permanently', 'Object moved permanently -- see URI list'),
    (302, 'FOUND', 'Found', 'Object moved temporarily -- see URI list'),
    (303, 'SEE_OTHER', 'See Other', 'Object moved -- see Method and URL list'),
    (304, 'NOT_MODIFIED', 'Not Modified', 'Document has not changed since given time'),
    (305, 'USE_PROXY', 'Use Proxy', 'You must use proxy specified in Location to access this resource'),
    (307, 'TEMPORARY_REDIRECT', 'Temporary Redirect', 'Object moved temporarily -- see URI list'),
    (308, 'PERMANENT_REDIRECT', 'Permanent Redirect', 'Object moved permanently -- see URI list'),
    (400, 'BAD_REQUEST', 'Bad Request', 'Bad request syntax or unsupported method'),
    (401, 'UNAUTHORIZED', 'Unauthorized', 'No permission -- see authorization schemes'),
    (402, 'PAYMENT_REQUIRED', 'Payment Required', 'No payment -- see charging schemes'),
    (403, 'FORBIDDEN', 'Forbidden', 'Request forbidden -- authorization will not help'),
    (404, 'NOT_FOUND', 'Not Found', 'Nothing matches the given URI'),
    (405, 'METHOD_NOT_ALLOWED', 'Method Not Allowed', 'Specified method is invalid for this resource'),
    (406, 'NOT_ACCEPTABLE', 'Not Acceptable', 'URI not available in preferred format'),
    (407, 'PROXY_AUTHENTICATION_REQUIRED', 'Proxy Authentication Required',
     'You must authenticate with this proxy before proceeding'),
    (408, 'REQUEST_TIMEOUT', 'Request Timeout', 'Request timed out; try again later'),
    (409, 'CONFLICT', 'Conflict', 'Request conflict'),
    (410, 'GONE', 'Gone', 'URI no longer exists and has been permanently removed'),
    (411, 'LENGTH_REQUIRED', 'Length Required', 'Client must specify Content-Length'),
    (412, 'PRECONDITION_FAILED', 'Precondition Failed', 'Precondition in headers is false'),
    (413, 'REQUEST_ENTITY_TOO_LARGE', 'Request Entity Too Large', 'Entity is too large'),
    (414, 'REQUEST_URI_TOO_LONG', 'Request-URI Too Long', 'URI is too long'),
    (415, 'UNSUPPORTED_MEDIA_TYPE', 'Unsupported Media Type', 'Entity body in unsupported format'),
    (416, 'REQUESTED_RANGE_NOT_SATISFIABLE', 'Requested Range Not Satisfiable',
     'Cannot satisfy request range'),
    (417, 'EXPECTATION_FAILED', 'Expectation Failed', 'Expect condition could not be satisfied'),
    (418, 'IM_A_TEAPOT', "I'm a Teapot", 'Server refuses to brew coffee because it is a teapot.'),
    (421, 'MISDIRECTED_REQUEST', 'Misdirected Request', 'Server is not able to produce a response'),
    (422, 'UNPROCESSABLE_ENTITY', 'Unprocessable Entity', ''),
    (423, 'LOCKED', 'Locked', ''),
    (424, 'FAILED_DEPENDENCY', 'Failed Dependency', ''),
    (425, 'TOO_EARLY', 'Too Early', ''),
    (426, 'UPGRADE_REQUIRED', 'Upgrade Required', ''),
    (428, 'PRECONDITION_REQUIRED', 'Precondition Required',
     'The origin server requires the request to be conditional'),
    (429, 'TOO_MANY_REQUESTS', 'Too Many Requests',
     'The user has sent too many requests in a given amount of time ("rate limiting")'),
    (431, 'REQUEST_HEADER_FIELDS_TOO_LARGE', 'Request Header Fields Too Large',
     'The server is unwilling to process the request because its header fields are too large'),
    (451, 'UNAVAILABLE_FOR_LEGAL_REASONS', 'Unavailable For Legal Reasons',
     'The server is denying access to the resource as a consequence of a legal demand'),
    (500, 'INTERNAL_SERVER_ERROR', 'Internal Server Error', 'Server got itself in trouble'),
    (501, 'NOT_IMPLEMENTED', 'Not Implemented', 'Server does not support this operation'),
    (502, 'BAD_GATEWAY', 'Bad Gateway', 'Invalid responses from another server/proxy'),
    (503, 'SERVICE_UNAVAILABLE', 'Service Unavailable',
     'The server cannot process the request due to a high load'),
    (504, 'GATEWAY_TIMEOUT', 'Gateway Timeout', 'The gateway server did not receive a timely response'),
    (505, 'HTTP_VERSION_NOT_SUPPORTED', 'HTTP Version Not Supported', 'Cannot fulfill request'),
    (506, 'VARIANT_ALSO_NEGOTIATES', 'Variant Also Negotiates', ''),
    (507, 'INSUFFICIENT_STORAGE', 'Insufficient Storage', ''),
    (508, 'LOOP_DETECTED', 'Loop Detected', ''),
    (510, 'NOT_EXTENDED', 'Not Extended', ''),
    (511, 'NETWORK_AUTHENTICATION_REQUIRED', 'Network Authentication Required',
     'The client needs to authenticate to gain network access'),
]

HTTPStatus = IntEnum('HTTPStatus', [(name, code) for code, name, _, _ in _STATUS])
_PHRASES = {code: (phrase, desc) for code, _, phrase, desc in _STATUS}
HTTPStatus.phrase = property(lambda self: _PHRASES[self.value][0])
HTTPStatus.description = property(lambda self: _PHRASES[self.value][1])
HTTPStatus.is_informational = property(lambda self: 100 <= self <= 199)
HTTPStatus.is_success = property(lambda self: 200 <= self <= 299)
HTTPStatus.is_redirection = property(lambda self: 300 <= self <= 399)
HTTPStatus.is_client_error = property(lambda self: 400 <= self <= 499)
HTTPStatus.is_server_error = property(lambda self: 500 <= self <= 599)


class HTTPMethod:
    CONNECT = 'CONNECT'
    DELETE = 'DELETE'
    GET = 'GET'
    HEAD = 'HEAD'
    OPTIONS = 'OPTIONS'
    PATCH = 'PATCH'
    POST = 'POST'
    PUT = 'PUT'
    TRACE = 'TRACE'
