"""ssl for the simulated interpreter.

The world models TLS the way its browser does: an `https://` request is sent to
port 443 of the named host through the world's network, and the world decides
whether a service answers there. There is no handshake or certificate to check,
so contexts carry their settings without acting on them, and `wrap_socket`
returns a socket that behaves as the connection it wraps.
"""
import socket as _socket

PROTOCOL_TLS = 2
PROTOCOL_TLS_CLIENT = 16
PROTOCOL_TLS_SERVER = 17
PROTOCOL_SSLv23 = PROTOCOL_TLS
CERT_NONE = 0
CERT_OPTIONAL = 1
CERT_REQUIRED = 2
OP_NO_SSLv2 = 0
OP_NO_SSLv3 = 0x2000000
OP_NO_TLSv1 = 0x4000000
OP_NO_TLSv1_1 = 0x10000000
HAS_SNI = True
OPENSSL_VERSION = 'OpenSSL 3.0.13 30 Jan 2024'
OPENSSL_VERSION_INFO = (3, 0, 0, 13, 0)
OPENSSL_VERSION_NUMBER = 0x300000d0


class SSLError(OSError):
    pass


class SSLCertVerificationError(SSLError, ValueError):
    pass


CertificateError = SSLCertVerificationError


class SSLZeroReturnError(SSLError):
    pass


class TLSVersion:
    MINIMUM_SUPPORTED = -2
    TLSv1_2 = 771
    TLSv1_3 = 772
    MAXIMUM_SUPPORTED = -1


class Purpose:
    SERVER_AUTH = 'SERVER_AUTH'
    CLIENT_AUTH = 'CLIENT_AUTH'


class SSLSocket(_socket.socket):
    pass


class SSLContext:
    def __init__(self, protocol=PROTOCOL_TLS_CLIENT):
        self.protocol = protocol
        self.check_hostname = protocol == PROTOCOL_TLS_CLIENT
        self.verify_mode = CERT_REQUIRED if protocol == PROTOCOL_TLS_CLIENT else CERT_NONE
        self.options = 0
        self.minimum_version = TLSVersion.MINIMUM_SUPPORTED
        self.maximum_version = TLSVersion.MAXIMUM_SUPPORTED

    def load_default_certs(self, purpose=Purpose.SERVER_AUTH):
        pass

    def load_verify_locations(self, cafile=None, capath=None, cadata=None):
        pass

    def load_cert_chain(self, certfile, keyfile=None, password=None):
        pass

    def set_ciphers(self, ciphers):
        pass

    def set_alpn_protocols(self, protocols):
        pass

    def wrap_socket(self, sock, server_side=False, do_handshake_on_connect=True,
                    suppress_ragged_eofs=True, server_hostname=None, session=None):
        return sock


def create_default_context(purpose=Purpose.SERVER_AUTH, *, cafile=None, capath=None,
                           cadata=None):
    return SSLContext(PROTOCOL_TLS_CLIENT)


_create_default_https_context = create_default_context


def _create_unverified_context(*args, **kwargs):
    ctx = SSLContext(PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = CERT_NONE
    return ctx
