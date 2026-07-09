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
#include <net/if.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#include <wasi/api.h>

__attribute__((import_module("host_process"), import_name("proc_closefrom")))
__wasi_errno_t host_proc_closefrom(__wasi_fd_t lowfd);

__attribute__((import_module("host_process"), import_name("proc_ppoll_v1")))
__wasi_errno_t host_proc_ppoll_v1(struct pollfd *fds, uint32_t nfds,
    int64_t timeout_sec, int64_t timeout_nsec, uint32_t sigmask_lo,
    uint32_t sigmask_hi, uint32_t has_sigmask, uint32_t *ret_ready);

__attribute__((import_module("host_net"), import_name("net_dns_query_rr_v1")))
__wasi_errno_t host_net_dns_query_rr_v1(const unsigned char *name,
    uint32_t name_len, uint32_t rrtype, unsigned char *out,
    uint32_t out_capacity, uint32_t *ret_len, uint32_t *ret_ttl,
    uint32_t *ret_flags);

/*
 * closefrom(2) — Solaris/FreeBSD/OpenBSD extension, glibc >= 2.34:
 * "closefrom() closes all open file descriptors greater than or equal to
 * lowfd" (closefrom(2), FreeBSD man pages).
 *
 * The host import closes virtual pipe, socket, passthrough, and dup descriptors,
 * including high synthetic fd numbers outside RLIMIT_NOFILE. WASI preopened
 * directory capabilities are runtime plumbing rather than guest-opened Linux
 * descriptors, so the host preserves those hidden handles while closing every
 * ordinary descriptor it owns. This retains path resolution and gives OpenSSH
 * the inherited-fd cleanup it gets from closefrom() on Linux/BSD without a
 * linear scan to RLIMIT_NOFILE. closefrom() has no error return; unexpected
 * close failures are still reported on stderr instead of being swallowed.
 */
void closefrom(int lowfd) {
	__wasi_errno_t host_error;

	if (lowfd < 0)
		lowfd = 0;
	host_error = host_proc_closefrom((__wasi_fd_t)lowfd);
	if (host_error != __WASI_ERRNO_SUCCESS)
		fprintf(stderr, "closefrom: host virtual fd cleanup: WASI errno %u\n",
		    (unsigned int)host_error);
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
 * The host_process.proc_ppoll_v1 import atomically swaps the cooperative
 * guest signal mask, polls through the same descriptor-aware host path, and
 * restores the old mask. Pending newly-unblocked caught signals interrupt the
 * wait with EINTR; signals blocked only by the temporary mask are delivered
 * after restoration without rewriting an already-successful poll result.
 */
int ppoll(struct pollfd *fds, nfds_t nfds, const struct timespec *timeout,
    const sigset_t *sigmask) {
	uint32_t ready = 0, mask_lo = 0, mask_hi = 0;
	int64_t timeout_sec = -1, timeout_nsec = -1;
	__wasi_errno_t error;

	if (nfds > UINT32_MAX) {
		errno = EINVAL;
		return -1;
	}
	if (timeout != NULL && (timeout->tv_sec < 0 || timeout->tv_nsec < 0 ||
	    timeout->tv_nsec > 999999999L)) {
		errno = EINVAL;
		return -1;
	}
	if (timeout != NULL) {
		timeout_sec = timeout->tv_sec;
		timeout_nsec = timeout->tv_nsec;
	}
	if (sigmask != NULL) {
		mask_lo = (uint32_t)sigmask->__bits[0];
		mask_hi = (uint32_t)sigmask->__bits[1];
	}
	error = host_proc_ppoll_v1(fds, (uint32_t)nfds, timeout_sec,
	    timeout_nsec, mask_lo, mask_hi, sigmask != NULL, &ready);
	if (error != __WASI_ERRNO_SUCCESS) {
		errno = error;
		return -1;
	}
	return (int)ready;
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
 *
 * Retained as a fallback for the historical fixed-identity runtime. Current
 * AgentOS builds leave this guard disabled and use the live host_user-backed
 * implementations from 0033-user-credentials.patch.
 */
#ifdef AGENTOS_WASI_LIBC_LEGACY_USER_SHIMS
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
#endif

/*
 * getgrent(3p) / setgrent(3p) / endgrent(3p) — enumerate the live projected
 * /etc/group database. Keeping the stream open between calls matches the
 * process-global cursor used by Linux libc, while setgrent rewinds it and
 * endgrent releases it. Physical records and member lists are bounded so a
 * hostile guest database cannot grow an unbounded libc allocation.
 *
 * Retained for the historical projected-/etc implementation. Current builds
 * use the bounded kernel account database from 0037-user-account-database.patch.
 */
#ifdef AGENTOS_WASI_LIBC_LEGACY_USER_SHIMS
#define AGENTOS_GROUP_LINE_MAX 4096
#define AGENTOS_GROUP_MEMBERS_MAX 128

static FILE *group_database;
static struct group group_entry;
static char group_line[AGENTOS_GROUP_LINE_MAX];
static char *group_members[AGENTOS_GROUP_MEMBERS_MAX];

/* Shared by the getgrgid/getgrnam wrappers installed in host_resource_user.c.
 * The reentrant result, its strings, and gr_mem vector all live in the caller's
 * buffer exactly as POSIX requires. */
int __agentos_group_lookup(const char *name, gid_t wanted_gid, int by_name,
    struct group *result_entry, char *buffer, size_t buffer_len,
    struct group **result) {
	char line[AGENTOS_GROUP_LINE_MAX];
	char *fields[4], *members[AGENTOS_GROUP_MEMBERS_MAX];
	char *cursor, *member, *save, *end, **member_vector;
	FILE *database;
	unsigned long gid;
	size_t record_len, member_offset, vector_offset, required;
	int field, member_count, close_error;

	if (result == NULL || result_entry == NULL || buffer == NULL ||
	    (by_name && name == NULL))
		return EINVAL;
	*result = NULL;
	database = fopen("/etc/group", "r");
	if (database == NULL)
		return errno;
	while (fgets(line, sizeof(line), database) != NULL) {
		if (strchr(line, '\n') == NULL && !feof(database)) {
			int ch;
			while ((ch = fgetc(database)) != '\n' && ch != EOF)
				;
			fclose(database);
			return ERANGE;
		}
		line[strcspn(line, "\r\n")] = '\0';
		record_len = strlen(line);
		fields[0] = line;
		cursor = line;
		for (field = 1; field < 4; field++) {
			cursor = strchr(cursor, ':');
			if (cursor == NULL)
				break;
			*cursor++ = '\0';
			fields[field] = cursor;
		}
		if (field != 4 || strchr(fields[3], ':') != NULL)
			continue;
		gid = strtoul(fields[2], &end, 10);
		if (*fields[2] == '\0' || *end != '\0' || gid > UINT32_MAX ||
		    (by_name ? strcmp(fields[0], name) != 0 : gid != wanted_gid))
			continue;
		member_count = 0;
		save = NULL;
		for (member = strtok_r(fields[3], ",", &save); member != NULL;
		    member = strtok_r(NULL, ",", &save)) {
			if (member_count >= AGENTOS_GROUP_MEMBERS_MAX - 1) {
				fclose(database);
				return ERANGE;
			}
			members[member_count++] = member;
		}
		vector_offset = (size_t)((((uintptr_t)buffer + record_len + 1 +
		    sizeof(char *) - 1) & ~(uintptr_t)(sizeof(char *) - 1)) -
		    (uintptr_t)buffer);
		required = vector_offset +
		    ((size_t)member_count + 1) * sizeof(char *);
		if (required > buffer_len) {
			fclose(database);
			return ERANGE;
		}
		memcpy(buffer, line, record_len + 1);
		member_vector = (char **)(void *)(buffer + vector_offset);
		for (int index = 0; index < member_count; index++) {
			member_offset = (size_t)(members[index] - line);
			member_vector[index] = buffer + member_offset;
		}
		member_vector[member_count] = NULL;
		result_entry->gr_name = buffer + (fields[0] - line);
		result_entry->gr_passwd = buffer + (fields[1] - line);
		result_entry->gr_gid = (gid_t)gid;
		result_entry->gr_mem = member_vector;
		close_error = fclose(database);
		if (close_error != 0)
			return errno;
		*result = result_entry;
		return 0;
	}
	if (ferror(database)) {
		int error = errno != 0 ? errno : EIO;
		fclose(database);
		return error;
	}
	if (fclose(database) != 0)
		return errno;
	return 0;
}

static struct group *read_group_entry(FILE *database) {
	char *fields[4], *cursor, *member, *save, *end;
	unsigned long gid;
	int field, member_count;

	while (fgets(group_line, sizeof(group_line), database) != NULL) {
		if (strchr(group_line, '\n') == NULL && !feof(database)) {
			int ch;
			while ((ch = fgetc(database)) != '\n' && ch != EOF)
				;
			errno = ERANGE;
			return NULL;
		}
		group_line[strcspn(group_line, "\r\n")] = '\0';
		fields[0] = group_line;
		cursor = group_line;
		for (field = 1; field < 4; field++) {
			cursor = strchr(cursor, ':');
			if (cursor == NULL)
				break;
			*cursor++ = '\0';
			fields[field] = cursor;
		}
		if (field != 4 || strchr(fields[3], ':') != NULL)
			continue;
		gid = strtoul(fields[2], &end, 10);
		if (*fields[2] == '\0' || *end != '\0' || gid > UINT32_MAX)
			continue;
		member_count = 0;
		save = NULL;
		for (member = strtok_r(fields[3], ",", &save); member != NULL;
		    member = strtok_r(NULL, ",", &save)) {
			if (member_count >= AGENTOS_GROUP_MEMBERS_MAX - 1) {
				errno = ERANGE;
				return NULL;
			}
			group_members[member_count++] = member;
		}
		group_members[member_count] = NULL;
		group_entry.gr_name = fields[0];
		group_entry.gr_passwd = fields[1];
		group_entry.gr_gid = (gid_t)gid;
		group_entry.gr_mem = group_members;
		return &group_entry;
	}
	return NULL;
}

struct group *getgrent(void) {
	if (group_database == NULL) {
		group_database = fopen("/etc/group", "r");
		if (group_database == NULL)
			return NULL;
	}
	return read_group_entry(group_database);
}

void setgrent(void) {
	if (group_database == NULL)
		group_database = fopen("/etc/group", "r");
	else {
		rewind(group_database);
		clearerr(group_database);
	}
}

void endgrent(void) {
	if (group_database != NULL) {
		if (fclose(group_database) != 0)
			fprintf(stderr, "endgrent: closing /etc/group: %s\n",
			    strerror(errno));
		group_database = NULL;
	}
}

/*
 * setgroups(2) / initgroups(3) — supplementary group management. The guest's
 * supplementary group set is fixed by the kernel (getgroups() reports it via
 * the host import from
 * std-patches/wasi-libc/0017-resource-limits-and-groups.patch). The fixed
 * guest identity has no CAP_SETGID equivalent, so Linux rejects every change,
 * including clearing the list or restating its current value, with EPERM.
 * initgroups(3) ultimately performs the same privileged operation.
 */
int setgroups(size_t size, const gid_t *list) {
	(void)size;
	(void)list;
	errno = EPERM;
	return -1;
}
#endif

int initgroups(const char *user, gid_t group) {
	gid_t set[1];

	(void)user;
	set[0] = group;
	return setgroups(1, set);
}

#define AGENTOS_DNS_RR_MAX 65536U
#define DNS_TYPE_PTR 12U
#define DNS_TYPE_SSHFP 44U
#define DNS_CLASS_IN 1U
#define DNS_RR_FLAG_DNSSEC 1U
#define DNS_RR_FLAG_NXDOMAIN 2U
#define DNS_RR_FLAG_NODATA 4U

/* Stable Linux interface identities for the VM's virtual network namespace.
 * Keep these libc APIs below applications so scope formatting and any other
 * upstream consumer see the same loopback identity. */
char *if_indextoname(unsigned int index, char *name) {
	if (name == NULL) {
		errno = EFAULT;
		return NULL;
	}
	if (index != 1) {
		errno = ENXIO;
		return NULL;
	}
	memcpy(name, "lo", 3);
	return name;
}

unsigned int if_nametoindex(const char *name) {
	if (name != NULL && strcmp(name, "lo") == 0)
		return 1;
	errno = ENODEV;
	return 0;
}

struct if_nameindex *if_nameindex(void) {
	struct if_nameindex *indexes;
	char *name;

	indexes = calloc(2, sizeof(*indexes) + 3);
	if (indexes == NULL)
		return NULL;
	name = (char *)(indexes + 2);
	memcpy(name, "lo", 3);
	indexes[0].if_index = 1;
	indexes[0].if_name = name;
	return indexes;
}

void if_freenameindex(struct if_nameindex *indexes) {
	free(indexes);
}

static uint32_t read_u32_le(const unsigned char *p) {
	return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
	    ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static __wasi_errno_t dns_query_rr(const char *name, uint32_t rrtype,
    unsigned char **reply, uint32_t *reply_len, uint32_t *ttl,
    uint32_t *flags) {
	unsigned char *buf;
	__wasi_errno_t error;

	*reply = NULL;
	*reply_len = *ttl = *flags = 0;
	if ((buf = malloc(AGENTOS_DNS_RR_MAX)) == NULL)
		return __WASI_ERRNO_NOMEM;
	error = host_net_dns_query_rr_v1((const unsigned char *)name,
	    (uint32_t)strlen(name), rrtype, buf, AGENTOS_DNS_RR_MAX,
	    reply_len, ttl, flags);
	if (error != __WASI_ERRNO_SUCCESS) {
		free(buf);
		return error;
	}
	if (*reply_len < sizeof(uint32_t) || *reply_len > AGENTOS_DNS_RR_MAX) {
		free(buf);
		return __WASI_ERRNO_ILSEQ;
	}
	*reply = buf;
	return __WASI_ERRNO_SUCCESS;
}

static int dns_first_rdata(const unsigned char *reply, uint32_t reply_len,
    const unsigned char **first, uint32_t *first_len) {
	uint32_t count, index, length, offset = 4;

	if (reply_len < 4)
		return 0;
	count = read_u32_le(reply);
	if (count == 0 || count > (reply_len - 4) / 4)
		return 0;
	*first = NULL;
	*first_len = 0;
	for (index = 0; index < count; index++) {
		if (reply_len - offset < 4)
			return 0;
		length = read_u32_le(reply + offset);
		offset += 4;
		if (length > reply_len - offset)
			return 0;
		if (index == 0) {
			*first = reply + offset;
			*first_len = length;
		}
		offset += length;
	}
	return offset == reply_len && *first_len != 0;
}

static int reverse_owner(const struct sockaddr *sa, char *owner,
    size_t owner_len) {
	if (sa->sa_family == AF_INET) {
		const unsigned char *a = (const unsigned char *)&
		    ((const struct sockaddr_in *)sa)->sin_addr;
		return snprintf(owner, owner_len, "%u.%u.%u.%u.in-addr.arpa.",
		    a[3], a[2], a[1], a[0]) < (int)owner_len ? 0 : -1;
	}
	if (sa->sa_family == AF_INET6) {
		const unsigned char *a = ((const struct sockaddr_in6 *)sa)->
		    sin6_addr.s6_addr;
		size_t used = 0;
		int i, n;
		if (IN6_IS_ADDR_V4MAPPED(
		    &((const struct sockaddr_in6 *)sa)->sin6_addr))
			return snprintf(owner, owner_len,
			    "%u.%u.%u.%u.in-addr.arpa.", a[15], a[14], a[13],
			    a[12]) < (int)owner_len ? 0 : -1;
		for (i = 15; i >= 0; i--) {
			n = snprintf(owner + used, owner_len - used, "%x.%x.",
			    a[i] & 0x0f, a[i] >> 4);
			if (n < 0 || (size_t)n >= owner_len - used)
				return -1;
			used += (size_t)n;
		}
		return snprintf(owner + used, owner_len - used, "ip6.arpa.") <
		    (int)(owner_len - used) ? 0 : -1;
	}
	return -1;
}

struct agentos_hosts_address {
	int family;
	uint32_t scope_id;
	char text[64];
};

int __agentos_hosts_lookup_forward(const char *host, int requested_family,
    struct agentos_hosts_address *addresses, size_t capacity, size_t *count,
    char *canonical_out, size_t canonical_len) {
	char line[4096], *comment, *save, *text, *canonical, *alias, *scope;
	unsigned char parsed[sizeof(struct in6_addr)];
	FILE *hosts;
	int family, matched;
	unsigned long scope_id;
	char *scope_end;

	if (count == NULL || addresses == NULL || canonical_out == NULL ||
	    canonical_len == 0)
		return -1;
	*count = 0;
	canonical_out[0] = '\0';
	if ((hosts = fopen("/etc/hosts", "r")) == NULL)
		return 0;
	while (fgets(line, sizeof(line), hosts) != NULL) {
		if (strchr(line, '\n') == NULL && !feof(hosts)) {
			int ch;
			while ((ch = fgetc(hosts)) != '\n' && ch != EOF)
				;
			continue;
		}
		if ((comment = strchr(line, '#')) != NULL)
			*comment = '\0';
		save = NULL;
		text = strtok_r(line, " \t\r\n", &save);
		canonical = strtok_r(NULL, " \t\r\n", &save);
		if (text == NULL || canonical == NULL)
			continue;
		matched = strcasecmp(canonical, host) == 0;
		while (!matched &&
		    (alias = strtok_r(NULL, " \t\r\n", &save)) != NULL)
			matched = strcasecmp(alias, host) == 0;
		if (!matched)
			continue;
		scope_id = 0;
		if ((scope = strchr(text, '%')) != NULL) {
			*scope++ = '\0';
			scope_id = strtoul(scope, &scope_end, 10);
			if (scope_end == scope || *scope_end != '\0')
				scope_id = if_nametoindex(scope);
			if (scope_id == 0 || scope_id > UINT32_MAX)
				continue;
		}
		if (inet_pton(AF_INET, text, parsed) == 1) {
			if (scope_id != 0)
				continue;
			family = AF_INET;
		} else if (inet_pton(AF_INET6, text, parsed) == 1) {
			family = AF_INET6;
		} else {
			continue;
		}
		if (requested_family != AF_UNSPEC && requested_family != family)
			continue;
		if (*count >= capacity || strlen(text) >= sizeof(addresses[0].text) ||
		    (*count == 0 && strlen(canonical) >= canonical_len)) {
			fclose(hosts);
			return -1;
		}
		addresses[*count].family = family;
		addresses[*count].scope_id = (uint32_t)scope_id;
		memcpy(addresses[*count].text, text, strlen(text) + 1);
		if (*count == 0)
			memcpy(canonical_out, canonical, strlen(canonical) + 1);
		(*count)++;
	}
	fclose(hosts);
	return 0;
}

int __agentos_service_port(const char *service, int socktype, int protocol) {
	char line[1024], *comment, *save, *name, *port_proto, *alias, *end;
	const char *wanted = socktype == SOCK_DGRAM || protocol == IPPROTO_UDP ?
	    "udp" : "tcp";
	FILE *services;
	unsigned long candidate;

	if ((services = fopen("/etc/services", "r")) == NULL)
		return -1;
	while (fgets(line, sizeof(line), services) != NULL) {
		if ((comment = strchr(line, '#')) != NULL)
			*comment = '\0';
		save = NULL;
		name = strtok_r(line, " \t\r\n", &save);
		port_proto = strtok_r(NULL, " \t\r\n", &save);
		if (name == NULL || port_proto == NULL)
			continue;
		candidate = strtoul(port_proto, &end, 10);
		if (end == port_proto || *end != '/' || candidate > 65535 ||
		    strcmp(end + 1, wanted) != 0)
			continue;
		if (strcmp(name, service) != 0) {
			while ((alias = strtok_r(NULL, " \t\r\n", &save)) != NULL)
				if (strcmp(alias, service) == 0)
					break;
			if (alias == NULL)
				continue;
		}
		fclose(services);
		return (int)candidate;
	}
	fclose(services);
	return -1;
}

static int hosts_name(const struct sockaddr *sa, char *name,
    size_t name_len) {
	unsigned char candidate[sizeof(struct in6_addr)];
	unsigned char v4[sizeof(struct in_addr)];
	const unsigned char *address;
	size_t address_len;
	char line[4096], *comment, *save, *text, *canonical, *scope_text;
	FILE *hosts;
	unsigned long candidate_scope;
	char *scope_end;

	if (sa->sa_family == AF_INET) {
		address = (const unsigned char *)&
		    ((const struct sockaddr_in *)sa)->sin_addr;
		address_len = sizeof(struct in_addr);
	} else if (sa->sa_family == AF_INET6) {
		address = ((const struct sockaddr_in6 *)sa)->sin6_addr.s6_addr;
		address_len = sizeof(struct in6_addr);
	} else {
		return 0;
	}
	/* musl's getnameinfo consults the live hosts database before DNS. Keep
	 * this file-backed and uncached so guest edits take effect immediately. */
	if ((hosts = fopen("/etc/hosts", "r")) == NULL)
		return 0;
	while (fgets(line, sizeof(line), hosts) != NULL) {
		/* A truncated physical line must not turn its continuation into a
		 * synthetic hosts entry. Discard the rest within a fixed bound. */
		if (strchr(line, '\n') == NULL && !feof(hosts)) {
			int ch;
			while ((ch = fgetc(hosts)) != '\n' && ch != EOF)
				;
			continue;
		}
		if ((comment = strchr(line, '#')) != NULL)
			*comment = '\0';
		save = NULL;
		text = strtok_r(line, " \t\r\n", &save);
		canonical = strtok_r(NULL, " \t\r\n", &save);
		if (text == NULL || canonical == NULL)
			continue;
		candidate_scope = 0;
		if ((scope_text = strchr(text, '%')) != NULL) {
			if (sa->sa_family != AF_INET6)
				continue;
			*scope_text++ = '\0';
			candidate_scope = strtoul(scope_text, &scope_end, 10);
			if (scope_end == scope_text || *scope_end != '\0')
				candidate_scope = if_nametoindex(scope_text);
			if (candidate_scope == 0 || candidate_scope > UINT32_MAX)
				continue;
		}
		if (sa->sa_family == AF_INET6 && candidate_scope !=
		    ((const struct sockaddr_in6 *)sa)->sin6_scope_id)
			continue;
		if (inet_pton(sa->sa_family, text, candidate) != 1) {
			/* musl normalizes IPv4 hosts-file entries to v4-mapped IPv6
			 * when the caller supplies an AF_INET6 mapped address. */
			if (sa->sa_family != AF_INET6 ||
			    !IN6_IS_ADDR_V4MAPPED(
			    &((const struct sockaddr_in6 *)sa)->sin6_addr) ||
			    inet_pton(AF_INET, text, v4) != 1)
				continue;
			memset(candidate, 0, 10);
			candidate[10] = candidate[11] = 0xff;
			memcpy(candidate + 12, v4, sizeof(v4));
		}
		if (memcmp(candidate, address, address_len) != 0)
			continue;
		if (strlen(canonical) >= name_len) {
			fclose(hosts);
			return -1;
		}
		memcpy(name, canonical, strlen(canonical) + 1);
		fclose(hosts);
		return 1;
	}
	fclose(hosts);
	return 0;
}

static int service_name(unsigned short port, int dgram, char *name,
    size_t name_len) {
	char line[1024], protocol[16], *comment, *save, *candidate, *port_proto;
	FILE *services;
	unsigned long candidate_port;
	char *end;

	/* Linux getnameinfo consults the NSS services database. The VM's
	 * file-backed equivalent is the live guest /etc/services, so package and
	 * user changes are observed instead of freezing a tiny built-in table. */
	if ((services = fopen("/etc/services", "r")) == NULL)
		return 0;
	while (fgets(line, sizeof(line), services) != NULL) {
		if ((comment = strchr(line, '#')) != NULL)
			*comment = '\0';
		save = NULL;
		candidate = strtok_r(line, " \t\r\n", &save);
		port_proto = strtok_r(NULL, " \t\r\n", &save);
		if (candidate == NULL || port_proto == NULL)
			continue;
		candidate_port = strtoul(port_proto, &end, 10);
		if (end == port_proto || *end != '/' || candidate_port > 65535)
			continue;
		if (snprintf(protocol, sizeof(protocol), "%s", end + 1) >=
		    (int)sizeof(protocol))
			continue;
		if (candidate_port != port ||
		    strcmp(protocol, dgram ? "udp" : "tcp") != 0)
			continue;
		if (strlen(candidate) >= name_len) {
			fclose(services);
			return -1;
		}
		memcpy(name, candidate, strlen(candidate) + 1);
		fclose(services);
		return 1;
	}
	fclose(services);
	return 0;
}

/*
 * getnameinfo(3p) — POSIX.1-2024 / RFC 3493 §6.2. Numeric formatting is
 * local; reverse lookups use bounded PTR RR queries through host_net and
 * fall back to the numeric address unless NI_NAMEREQD was requested.
 */
int getnameinfo(const struct sockaddr *restrict sa, socklen_t salen,
    char *restrict host, socklen_t hostlen,
    char *restrict serv, socklen_t servlen, int flags) {
	const int valid_flags = NI_NUMERICHOST | NI_NUMERICSERV | NI_NOFQDN |
	    NI_NAMEREQD | NI_DGRAM | NI_NUMERICSCOPE;
	char buf[INET6_ADDRSTRLEN + 24], owner[96];
	const void *address;
	uint32_t reply_len, ttl, rrflags, rdata_len;
	unsigned char *reply = NULL;
	const unsigned char *rdata;
	__wasi_errno_t dns_error;
	unsigned short port;
	uint32_t scope_id = 0;
	size_t length;
	int resolved_name = 0;

	if (sa == NULL)
		return EAI_FAIL;
	if ((flags & ~valid_flags) != 0)
		return EAI_BADFLAGS;
	switch (sa->sa_family) {
	case AF_INET: {
		const struct sockaddr_in *sin = (const struct sockaddr_in *)sa;
		if (salen < (socklen_t)sizeof(*sin))
			return EAI_FAMILY;
		address = &sin->sin_addr;
		port = ntohs(sin->sin_port);
		break;
	}
	case AF_INET6: {
		const struct sockaddr_in6 *sin6 =
		    (const struct sockaddr_in6 *)sa;
		if (salen < (socklen_t)sizeof(*sin6))
			return EAI_FAMILY;
		address = &sin6->sin6_addr;
		scope_id = sin6->sin6_scope_id;
		port = ntohs(sin6->sin6_port);
		break;
	}
	default:
		return EAI_FAMILY;
	}
	if (host != NULL && hostlen != 0) {
		/* Track writes explicitly; failed lookups leave caller bytes untouched. */
		if (inet_ntop(sa->sa_family, address, buf, sizeof(buf)) == NULL)
			return EAI_FAIL;
		if (sa->sa_family == AF_INET6 && scope_id != 0) {
			char interface[IF_NAMESIZE];
			const char *scope = NULL;
			const struct in6_addr *in6 =
			    &((const struct sockaddr_in6 *)sa)->sin6_addr;
			if ((flags & NI_NUMERICSCOPE) == 0 &&
			    (IN6_IS_ADDR_LINKLOCAL(in6) ||
			    IN6_IS_ADDR_MC_LINKLOCAL(in6)))
				scope = if_indextoname(scope_id, interface);
			length = strlen(buf);
			if (scope != NULL) {
				if (snprintf(buf + length, sizeof(buf) - length, "%%%s",
				    scope) >= (int)(sizeof(buf) - length))
					return EAI_OVERFLOW;
			} else if (snprintf(buf + length, sizeof(buf) - length, "%%%u",
			    scope_id) >= (int)(sizeof(buf) - length)) {
				return EAI_OVERFLOW;
			}
		}
		/* Alpine/musl accepts NI_NOFQDN but intentionally treats it as a
		 * no-op. Preserve the full canonical hosts/PTR name here too. */
		if ((flags & NI_NUMERICHOST) == 0) {
			int hosts_result = hosts_name(sa, host, (size_t)hostlen);
			if (hosts_result < 0)
				return EAI_OVERFLOW;
			resolved_name = hosts_result > 0;
		}
		if (!resolved_name && (flags & NI_NUMERICHOST) == 0 &&
		    reverse_owner(sa, owner, sizeof(owner)) == 0) {
			dns_error = dns_query_rr(owner, DNS_TYPE_PTR, &reply,
			    &reply_len, &ttl, &rrflags);
			if (dns_error == __WASI_ERRNO_SUCCESS) {
				if (dns_first_rdata(reply, reply_len, &rdata,
				    &rdata_len) && memchr(rdata, '\0', rdata_len) == NULL) {
					length = rdata_len;
					if (rdata[length - 1] == '.')
						length--;
					if (length != 0) {
						if (length >= (size_t)hostlen) {
							free(reply);
							return EAI_OVERFLOW;
						}
						memcpy(host, rdata, length);
						host[length] = '\0';
						resolved_name = 1;
						free(reply);
						reply = NULL;
					}
				}
				free(reply);
				reply = NULL;
			}
		}
		if (!resolved_name) {
			if (flags & NI_NAMEREQD)
				return EAI_NONAME;
			length = strlen(buf);
			if (length >= (size_t)hostlen)
				return EAI_OVERFLOW;
			memcpy(host, buf, length + 1);
		}
	}

	if (serv != NULL && servlen != 0) {
		const char *name = NULL;
		char servicebuf[64];
		char portbuf[8];
		int service_result = 0;
		if ((flags & NI_NUMERICSERV) == 0)
			service_result = service_name(port,
			    (flags & NI_DGRAM) != 0, servicebuf,
			    sizeof(servicebuf));
		if (service_result < 0)
			return EAI_OVERFLOW;
		if (service_result > 0)
			name = servicebuf;
		if (name == NULL) {
			int n = snprintf(portbuf, sizeof(portbuf), "%u",
			    (unsigned int)port);
			if (n < 0)
				return EAI_FAIL;
			name = portbuf;
		}
		if (strlen(name) >= (size_t)servlen)
			return EAI_OVERFLOW;
		memcpy(serv, name, strlen(name) + 1);
	}

	return 0;
}

/* OpenBSD getrrsetbyname(3), backed by bounded host_net SSHFP queries. */
int getrrsetbyname(const char *hostname, unsigned int rdclass,
    unsigned int rdtype, unsigned int flags, struct rrsetinfo **res) {
	struct rrsetinfo *rrset = NULL;
	unsigned char *reply = NULL;
	uint32_t reply_len, ttl, rrflags, count, length, offset;
	__wasi_errno_t error;
	unsigned int i;

	if (res == NULL || hostname == NULL || *hostname == '\0')
		return ERRSET_INVAL;
	*res = NULL;
	if (rdclass != DNS_CLASS_IN || rdtype != DNS_TYPE_SSHFP || flags != 0)
		return ERRSET_INVAL;
	error = dns_query_rr(hostname, rdtype, &reply, &reply_len, &ttl,
	    &rrflags);
	if (error == __WASI_ERRNO_NOMEM)
		return ERRSET_NOMEMORY;
	if (error == __WASI_ERRNO_NOENT)
		return ERRSET_NONAME;
	if (error != __WASI_ERRNO_SUCCESS)
		return ERRSET_FAIL;
	if (rrflags & DNS_RR_FLAG_NXDOMAIN) {
		free(reply);
		return ERRSET_NONAME;
	}
	if (rrflags & DNS_RR_FLAG_NODATA) {
		free(reply);
		return ERRSET_NODATA;
	}
	count = read_u32_le(reply);
	if (count == 0) {
		free(reply);
		return ERRSET_NODATA;
	}
	if (count > (reply_len - 4) / 4)
		goto malformed;
	if ((rrset = calloc(1, sizeof(*rrset))) == NULL ||
	    (rrset->rri_name = strdup(hostname)) == NULL ||
	    (rrset->rri_rdatas = calloc(count,
	    sizeof(*rrset->rri_rdatas))) == NULL)
		goto nomem;
	rrset->rri_flags = (rrflags & DNS_RR_FLAG_DNSSEC) != 0 ?
	    RRSET_VALIDATED : 0;
	rrset->rri_rdclass = rdclass;
	rrset->rri_rdtype = rdtype;
	rrset->rri_ttl = ttl;
	rrset->rri_nrdatas = count;
	offset = 4;
	for (i = 0; i < count; i++) {
		if (reply_len - offset < 4)
			goto malformed;
		length = read_u32_le(reply + offset);
		offset += 4;
		if (length > reply_len - offset)
			goto malformed;
		rrset->rri_rdatas[i].rdi_length = length;
		if ((rrset->rri_rdatas[i].rdi_data = malloc(length)) == NULL)
			goto nomem;
		memcpy(rrset->rri_rdatas[i].rdi_data, reply + offset, length);
		offset += length;
	}
	if (offset != reply_len)
		goto malformed;
	free(reply);
	*res = rrset;
	return ERRSET_SUCCESS;

 nomem:
	free(reply);
	freerrset(rrset);
	return ERRSET_NOMEMORY;
 malformed:
	free(reply);
	freerrset(rrset);
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
