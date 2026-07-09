/*
 * OpenSSH compatibility stubs for the agentOS wasm32-wasip1 sysroot.
 *
 * Declarations for both functions live in the patched sysroot headers
 * (std-patches/wasi-libc/0029-openssh-compat-header-surface.patch).
 */

#include <arpa/inet.h>
#include <errno.h>
#include <grp.h>
#include <limits.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#include <wasi/api.h>

__attribute__((import_module("host_process"), import_name("proc_closefrom")))
static __wasi_errno_t host_proc_closefrom(__wasi_fd_t lowfd);

/*
 * closefrom(2) — Solaris/FreeBSD/OpenBSD extension, glibc >= 2.34:
 * "closefrom() closes all open file descriptors greater than or equal to
 * lowfd" (closefrom(2), FreeBSD man pages).
 *
 * The host import closes virtual pipe, socket, passthrough, and dup descriptors,
 * including high synthetic fd numbers outside RLIMIT_NOFILE. WASI preopened
 * directory capabilities are runtime plumbing rather than guest-opened Linux
 * descriptors, so the local pass preserves those hidden handles while closing
 * every ordinary descriptor in the RLIMIT_NOFILE range. This retains path
 * resolution and gives OpenSSH the inherited-fd cleanup it gets from
 * closefrom() on Linux/BSD. closefrom() has no error return; unexpected close
 * failures are still reported on stderr instead of being swallowed.
 */
void closefrom(int lowfd) {
	struct rlimit limit;
	__wasi_errno_t host_error;
	int fd;

	if (lowfd < 0)
		lowfd = 0;
	host_error = host_proc_closefrom((__wasi_fd_t)lowfd);
	if (host_error != __WASI_ERRNO_SUCCESS)
		fprintf(stderr, "closefrom: host virtual fd cleanup: WASI errno %u\n",
		    (unsigned int)host_error);
	if (getrlimit(RLIMIT_NOFILE, &limit) < 0) {
		fprintf(stderr, "closefrom: getrlimit(RLIMIT_NOFILE): %s\n",
		    strerror(errno));
		return;
	}
	for (fd = lowfd; (rlim_t)fd < limit.rlim_cur && fd < INT_MAX; fd++) {
		__wasi_prestat_t prestat;
		if (__wasi_fd_prestat_get((__wasi_fd_t)fd, &prestat) ==
		    __WASI_ERRNO_SUCCESS)
			continue;
		if (close(fd) < 0 && errno != EBADF)
			fprintf(stderr, "closefrom: close(%d): %s\n", fd,
			    strerror(errno));
	}
}

/*
 * socketpair(3p) — POSIX.1-2024: "the socketpair() function shall create an
 * unnamed pair of connected sockets". The agentOS kernel has no socketpair
 * syscall (no AF_UNIX datagram plumbing between two anonymous fds), so this
 * honestly fails with ENOSYS, which POSIX permits via the EOPNOTSUPP/EAFNOSUPPORT
 * family for unsupported domains; ENOSYS names the real condition ("the
 * function is not supported by this implementation").
 *
 * The stub exists so ported tools link: OpenSSH references socketpair() from
 * cold paths the batch client never takes — sshconnect.c
 * ssh_proxy_fdpass_connect() (ProxyCommand/ProxyUseFdpass, RFC 4251 §4.4
 * "proxies and gateways" territory) and channels/mux code. If one of those
 * paths is ever reached, the caller gets a real errno instead of a link
 * failure or a silent fake socket.
 */
int socketpair(int domain, int type, int protocol, int sv[2]) {
	(void)domain;
	(void)type;
	(void)protocol;
	(void)sv;
	errno = ENOSYS;
	return -1;
}

/*
 * ppoll(2) — Linux/ppoll(3p, POSIX.1-2024): poll with a struct timespec
 * timeout and an atomically-applied signal mask. The patched sysroot routes
 * poll() through the host_net.net_poll import
 * (std-patches/wasi-libc/0023-host-net-read-write-sockets.patch), which
 * understands host-net sockets, kernel pipes/PTYs, and the runtime's
 * high-numbered dup'd fds. Implement ppoll as a wrapper over that poll so
 * link probes (AC_CHECK_FUNCS(ppoll)) succeed and ported tools skip their
 * own replacements — OpenSSH's openbsd-compat/bsd-poll.c fallback is built
 * on pselect() and fails with EINVAL for any fd >= FD_SETSIZE, which the
 * runtime's virtual dup fds (>= 0x100000, see the fd_dup_min host import)
 * always are.
 *
 * The sigmask parameter is deliberately ignored: this runtime has no
 * asynchronous signal delivery to race against — signals are cooperative
 * and dispatched at poll boundaries inside net_poll (see
 * std-patches/wasi-libc/0011-sigaction.patch), so the atomic
 * "swap-mask-and-wait" that ppoll(2) exists to provide has no observable
 * effect here.
 */
int ppoll(struct pollfd *fds, nfds_t nfds, const struct timespec *timeout,
    const sigset_t *sigmask) {
	int timeout_ms;

	(void)sigmask;
	if (timeout == NULL) {
		timeout_ms = -1;
	} else if (timeout->tv_sec < 0 || timeout->tv_nsec < 0 ||
	    timeout->tv_nsec > 999999999L) {
		errno = EINVAL;
		return -1;
	} else if (timeout->tv_sec > (time_t)(INT_MAX / 1000 - 1)) {
		timeout_ms = INT_MAX;
	} else {
		timeout_ms = (int)(timeout->tv_sec * 1000 +
		    (timeout->tv_nsec + 999999L) / 1000000L);
	}
	return poll(fds, nfds, timeout_ms);
}

/*
 * set*id family — unprivileged-process subset of POSIX setuid(3p) /
 * setgid(3p) / setreuid(3p) and Linux setresuid(2). Guest process identity is
 * fixed by the kernel (getuid()/getgid() come from the host_user import, see
 * std-patches/wasi-libc/0005-user-identity.patch); there is no privilege to
 * raise or drop. POSIX: an unprivileged process may set its ids only to
 * values it already holds (real, effective, or saved — all identical here),
 * and "the setuid() function shall fail [EPERM] ... if uid does not match the
 * real user ID or the saved set-user-ID". So self-assignment succeeds as a
 * no-op and any other target fails with EPERM. OpenSSH's uidswap.c relies on
 * exactly the self-assignment case (setuid(getuid()) etc.) to guarantee it
 * holds no elevated privileges.
 */
static int set_fixed_id(unsigned int requested, unsigned int current) {
	if (requested == current)
		return 0;
	errno = EPERM;
	return -1;
}

/* (uid_t)-1 / (gid_t)-1 means "leave unchanged" per setreuid(3p)/setresuid(2). */
static int set_fixed_id_opt(unsigned int requested, unsigned int current) {
	if (requested == (unsigned int)-1)
		return 0;
	return set_fixed_id(requested, current);
}

int setuid(uid_t uid) {
	return set_fixed_id(uid, getuid());
}

int seteuid(uid_t euid) {
	return set_fixed_id(euid, geteuid());
}

int setgid(gid_t gid) {
	return set_fixed_id(gid, getgid());
}

int setegid(gid_t egid) {
	return set_fixed_id(egid, getegid());
}

int setreuid(uid_t ruid, uid_t euid) {
	if (set_fixed_id_opt(ruid, getuid()) < 0)
		return -1;
	return set_fixed_id_opt(euid, geteuid());
}

int setregid(gid_t rgid, gid_t egid) {
	if (set_fixed_id_opt(rgid, getgid()) < 0)
		return -1;
	return set_fixed_id_opt(egid, getegid());
}

int setresuid(uid_t ruid, uid_t euid, uid_t suid) {
	if (set_fixed_id_opt(ruid, getuid()) < 0)
		return -1;
	if (set_fixed_id_opt(euid, geteuid()) < 0)
		return -1;
	return set_fixed_id_opt(suid, getuid());
}

int setresgid(gid_t rgid, gid_t egid, gid_t sgid) {
	if (set_fixed_id_opt(rgid, getgid()) < 0)
		return -1;
	if (set_fixed_id_opt(egid, getegid()) < 0)
		return -1;
	return set_fixed_id_opt(sgid, getgid());
}

int getresuid(uid_t *ruid, uid_t *euid, uid_t *suid) {
	if (ruid)
		*ruid = getuid();
	if (euid)
		*euid = geteuid();
	if (suid)
		*suid = getuid();
	return 0;
}

int getresgid(gid_t *rgid, gid_t *egid, gid_t *sgid) {
	if (rgid)
		*rgid = getgid();
	if (egid)
		*egid = getegid();
	if (sgid)
		*sgid = getgid();
	return 0;
}

/*
 * getgrent(3p) / setgrent(3p) / endgrent(3p) — POSIX group database
 * enumeration. The agentOS runtime resolves groups by id/name through host
 * imports (std-patches/wasi-libc/0025-group-lookup-compat.patch provides
 * getgrgid/getgrnam); there is no enumerable /etc/group database, so
 * enumeration reports immediate end-of-database: POSIX getgrent(3p) "shall
 * return a null pointer ... on end-of-file". setgrent/endgrent rewind/close
 * that (empty) enumeration and are no-ops. Having real symbols keeps ported
 * tools off their compat macro replacements (OpenSSH's bsd-misc.h
 * `#define endgrent() do { } while(0)` collides with the grp.h prototype).
 */
struct group *getgrent(void) {
	return NULL;
}

void setgrent(void) {
}

void endgrent(void) {
}

/*
 * setgroups(2) / initgroups(3) — supplementary group management. The guest's
 * supplementary group set is fixed by the kernel (getgroups() reports it via
 * the host import from
 * std-patches/wasi-libc/0017-resource-limits-and-groups.patch), and POSIX
 * reserves changing it to privileged processes ("appropriate privileges",
 * setgroups(2): "EPERM The calling process ... does not have the CAP_SETGID
 * capability"). Requests that restate the current single-group set succeed
 * as no-ops; anything else fails with EPERM. initgroups(3) is glibc/BSD
 * "read /etc/group and call setgroups" — with the enumerable group database
 * empty (see getgrent above) its computed set is exactly {basegid}. OpenSSH
 * misc.c subprocess() only calls initgroups when geteuid() == 0.
 */
int setgroups(size_t size, const gid_t *list) {
	if (size == 0 || (size == 1 && list != NULL && list[0] == getgid()))
		return 0;
	errno = EPERM;
	return -1;
}

int initgroups(const char *user, gid_t group) {
	gid_t set[1];

	(void)user;
	set[0] = group;
	return setgroups(1, set);
}

/*
 * getnameinfo(3p) — POSIX.1-2024 / RFC 3493 §6.2 ("Socket Address Structure
 * to Node Name and Service Name"). wasi-libc's <netdb.h> (un-omitted by the
 * sockets patch) declares getnameinfo but the sysroot never defined it; the
 * host_net-backed host_socket.o only implements getaddrinfo. This is the
 * numeric subset: it formats addresses with inet_ntop and ports with
 * snprintf. Reverse DNS is not available in the runtime (there is no
 * host-side reverse-lookup import), so NI_NAMEREQD fails with EAI_NONAME —
 * exactly what POSIX prescribes when "the name of the host cannot be
 * located" — and name-preferred lookups degrade to the numeric form, the
 * same observable behavior as a Linux host with no working reverse DNS.
 * OpenSSH uses getnameinfo() pervasively (canonical host strings for
 * known_hosts, log messages) with NI_NUMERICHOST/NI_NUMERICSERV on the paths
 * the batch client exercises.
 */
int getnameinfo(const struct sockaddr *restrict sa, socklen_t salen,
    char *restrict host, socklen_t hostlen,
    char *restrict serv, socklen_t servlen, int flags) {
	char buf[INET6_ADDRSTRLEN];
	unsigned short port;

	if (sa == NULL)
		return EAI_FAIL;

	switch (sa->sa_family) {
	case AF_INET: {
		const struct sockaddr_in *sin = (const struct sockaddr_in *)sa;
		if (salen < (socklen_t)sizeof(*sin))
			return EAI_FAMILY;
		if (host != NULL && hostlen > 0) {
			if (flags & NI_NAMEREQD)
				return EAI_NONAME;
			if (inet_ntop(AF_INET, &sin->sin_addr, buf,
			    sizeof(buf)) == NULL)
				return EAI_FAIL;
			if (strlen(buf) >= (size_t)hostlen)
				return EAI_OVERFLOW;
			memcpy(host, buf, strlen(buf) + 1);
		}
		port = ntohs(sin->sin_port);
		break;
	}
	case AF_INET6: {
		const struct sockaddr_in6 *sin6 =
		    (const struct sockaddr_in6 *)sa;
		if (salen < (socklen_t)sizeof(*sin6))
			return EAI_FAMILY;
		if (host != NULL && hostlen > 0) {
			if (flags & NI_NAMEREQD)
				return EAI_NONAME;
			if (inet_ntop(AF_INET6, &sin6->sin6_addr, buf,
			    sizeof(buf)) == NULL)
				return EAI_FAIL;
			if (strlen(buf) >= (size_t)hostlen)
				return EAI_OVERFLOW;
			memcpy(host, buf, strlen(buf) + 1);
		}
		port = ntohs(sin6->sin6_port);
		break;
	}
	default:
		return EAI_FAMILY;
	}

	if (serv != NULL && servlen > 0) {
		/* No services database in the VM; numeric only (as if
		 * NI_NUMERICSERV were always set). */
		char portbuf[8];
		int n = snprintf(portbuf, sizeof(portbuf), "%u",
		    (unsigned int)port);
		if (n < 0 || n >= (int)servlen)
			return EAI_OVERFLOW;
		memcpy(serv, portbuf, (size_t)n + 1);
	}

	return 0;
}

/*
 * getrrsetbyname(3) / freerrset(3) — OpenBSD DNS RRset query API, declared in
 * the patched <netdb.h>. OpenSSH's dns.c uses it to fetch SSHFP records for
 * VerifyHostKeyDNS (RFC 4255 "Using DNS to Securely Publish Secure Shell
 * (SSH) Key Fingerprints"). This sysroot has no resolver library (res_query
 * and friends are not built; DNS goes through the host getaddrinfo import),
 * so raw RRset queries are unsupported: fail with ERRSET_FAIL ("general
 * failure", getrrsetbyname(3) RETURN VALUES). dns.c maps that to its normal
 * "DNS lookup error" diagnostic and host-key verification proceeds via
 * known_hosts, exactly like a host whose resolver cannot reach a nameserver.
 */
int getrrsetbyname(const char *hostname, unsigned int rdclass,
    unsigned int rdtype, unsigned int flags, struct rrsetinfo **res) {
	(void)hostname;
	(void)rdclass;
	(void)rdtype;
	(void)flags;
	if (res)
		*res = NULL;
	return ERRSET_FAIL;
}

void freerrset(struct rrsetinfo *rrset) {
	unsigned int i;

	if (rrset == NULL)
		return;
	/* Mirrors OpenBSD lib/libc/net/getrrsetbyname.c freerrset(). */
	if (rrset->rri_rdatas) {
		for (i = 0; i < rrset->rri_nrdatas; i++)
			free(rrset->rri_rdatas[i].rdi_data);
		free(rrset->rri_rdatas);
	}
	if (rrset->rri_sigs) {
		for (i = 0; i < rrset->rri_nsigs; i++)
			free(rrset->rri_sigs[i].rdi_data);
		free(rrset->rri_sigs);
	}
	free(rrset->rri_name);
	free(rrset);
}
