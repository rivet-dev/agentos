const char* strerrno(int errnum)
{
	switch ( errnum )
	{
	case 0: return "errno == 0";
	case E2BIG: return "E2BIG";
	case EACCES: return "EACCES";
	case EADDRINUSE: return "EADDRINUSE";
	case EADDRNOTAVAIL: return "EADDRNOTAVAIL";
	case EAFNOSUPPORT: return "EAFNOSUPPORT";
#if EWOULDBLOCK != EAGAIN
	case EAGAIN: return "EAGAIN";
#endif
	case EALREADY: return "EALREADY";
	case EBADF: return "EBADF";
	case EBADMSG: return "EBADMSG";
	case EBUSY: return "EBUSY";
	case ECANCELED: return "ECANCELED";
	case ECHILD: return "ECHILD";
	case ECONNABORTED: return "ECONNABORTED";
	case ECONNREFUSED: return "ECONNREFUSED";
	case ECONNRESET: return "ECONNRESET";
	case EDEADLK: return "EDEADLK";
	case EDESTADDRREQ: return "EDESTADDRREQ";
	case EDOM: return "EDOM";
	case EDQUOT: return "EDQUOT";
	case EEXIST: return "EEXIST";
	case EFAULT: return "EFAULT";
	case EFBIG: return "EFBIG";
	case EHOSTUNREACH: return "EHOSTUNREACH";
	case EIDRM: return "EIDRM";
	case EILSEQ: return "EILSEQ";
	case EINPROGRESS: return "EINPROGRESS";
	case EINTR: return "EINTR";
	case EINVAL: return "EINVAL";
	case EIO: return "EIO";
	case EISCONN: return "EISCONN";
	case EISDIR: return "EISDIR";
	case ELOOP: return "ELOOP";
	case EMFILE: return "EMFILE";
	case EMLINK: return "EMLINK";
	case EMSGSIZE: return "EMSGSIZE";
#ifdef EMULTIHOP
	case EMULTIHOP: return "EMULTIHOP";
#endif
	case ENAMETOOLONG: return "ENAMETOOLONG";
	case ENETDOWN: return "ENETDOWN";
	case ENETRESET: return "ENETRESET";
	case ENETUNREACH: return "ENETUNREACH";
	case ENFILE: return "ENFILE";
	case ENOBUFS: return "ENOBUFS";
	case ENODEV: return "ENODEV";
	case ENOENT: return "ENOENT";
	case ENOEXEC: return "ENOEXEC";
	case ENOLCK: return "ENOLCK";
#ifdef ENOLINK
	case ENOLINK: return "ENOLINK";
#endif
	case ENOMEM: return "ENOMEM";
	case ENOMSG: return "ENOMSG";
	case ENOPROTOOPT: return "ENOPROTOOPT";
	case ENOSPC: return "ENOSPC";
	case ENOSYS: return "ENOSYS";
	case ENOTCONN: return "ENOTCONN";
	case ENOTDIR: return "ENOTDIR";
#if ENOTEMPTY != EEXIST
	case ENOTEMPTY: return "ENOTEMPTY";
#endif
#ifdef ENOTRECOVERABLE
	case ENOTRECOVERABLE: return "ENOTRECOVERABLE";
#endif
	case ENOTSOCK: return "ENOTSOCK";
	case ENOTSUP: return "ENOTSUP";
	case ENOTTY: return "ENOTTY";
	case ENXIO: return "ENXIO";
#if EOPNOTSUPP != ENOTSUP
	case EOPNOTSUPP: return "ENOTSUP";
#endif
	case EOVERFLOW: return "EOVERFLOW";
#ifdef EOWNERDEAD
	case EOWNERDEAD: return "EOWNERDEAD";
#endif
	case EPERM: return "EPERM";
#ifdef EPFNOSUPPORT
	case EPFNOSUPPORT: return "EPFNOSUPPORT";
#endif
	case EPIPE: return "EPIPE";
	case EPROTO: return "EPROTO";
	case EPROTONOSUPPORT: return "EPROTONOSUPPORT";
	case EPROTOTYPE: return "EPROTOTYPE";
	case ERANGE: return "ERANGE";
	case EROFS: return "EROFS";
#ifdef ESOCKTNOSUPPORT
	case ESOCKTNOSUPPORT: return "ESOCKTNOSUPPORT";
#endif
	case ESPIPE: return "ESPIPE";
	case ESRCH: return "ESRCH";
	case ESTALE: return "ESTALE";
	case ETIMEDOUT: return "ETIMEDOUT";
	case ETXTBSY: return "ETXTBSY";
	case EWOULDBLOCK: return "EWOULDBLOCK";
	case EXDEV: return "EXDEV";

	default: return strerror(errnum);
	}
}

__attribute__((unused))
static void test_vwarnc(int errnum, const char* fmt, va_list ap)
{
	if ( fmt )
	{
		vfprintf(stderr, fmt, ap);
		fputs(": ", stderr);
	}
	fprintf(stderr, "%s\n", strerrno(errnum));
}

__attribute__((unused))
static void test_vwarn(const char* fmt, va_list ap)
{
	test_vwarnc(errno, fmt, ap);
}

__attribute__((unused))
static void test_warn(const char* fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	test_vwarn(fmt, ap);
	va_end(ap);
}

__attribute__((unused))
static void test_vwarnx(const char* fmt, va_list ap)
{
	if ( fmt )
		vfprintf(stderr, fmt, ap);
	fputc('\n', stderr);
}

__attribute__((unused))
static void test_warnx(const char* fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	test_vwarnx(fmt, ap);
	va_end(ap);
}

__attribute__((unused))
static void test_verr(int exitcode, const char* fmt, va_list ap)
{
	test_vwarn(fmt, ap);
	exit(exitcode);
}

__attribute__((unused))
static void test_err(int exitcode, const char* fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	test_verr(exitcode, fmt, ap);
	va_end(ap);
}

__attribute__((unused))
static void test_verrx(int exitcode, const char* fmt, va_list ap)
{
	test_vwarnx(fmt, ap);
	exit(exitcode);
}

__attribute__((unused))
static void test_errx(int exitcode, const char* fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	test_verrx(exitcode, fmt, ap);
	va_end(ap);
}

#define err test_err
#define errc test_errc
#define errx test_errx
#define verr test_err
#define verrc test_errc
#define verrx test_errx
#define warn test_warn
#define warnc test_warnc
#define warnx test_warnx
#define vwarn test_warn
#define vwarnc test_warnc
#define vwarnx test_warnx
