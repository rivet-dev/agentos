#include <arpa/inet.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>

/* POSIX getnameinfo(3p): https://pubs.opengroup.org/onlinepubs/9799919799/functions/getnameinfo.html */
int getnameinfo(const struct sockaddr *address, socklen_t address_length,
                char *host, socklen_t host_length, char *service,
                socklen_t service_length, int flags) {
    const void *numeric_address;
    in_port_t port;
    int family;

    if (address == NULL)
        return EAI_FAIL;
    family = address->sa_family;
    if (family == AF_INET && address_length >= sizeof(struct sockaddr_in)) {
        const struct sockaddr_in *ipv4 = (const struct sockaddr_in *)address;
        numeric_address = &ipv4->sin_addr;
        port = ipv4->sin_port;
    } else if (family == AF_INET6 && address_length >= sizeof(struct sockaddr_in6)) {
        const struct sockaddr_in6 *ipv6 = (const struct sockaddr_in6 *)address;
        numeric_address = &ipv6->sin6_addr;
        port = ipv6->sin6_port;
    } else {
        return EAI_FAMILY;
    }

    if (host != NULL && host_length > 0) {
        if (inet_ntop(family, numeric_address, host, host_length) == NULL)
            return EAI_OVERFLOW;
    } else if ((flags & NI_NAMEREQD) != 0) {
        return EAI_NONAME;
    }
    if (service != NULL && service_length > 0) {
        const int written = snprintf(service, service_length, "%u", ntohs(port));
        if (written < 0 || (socklen_t)written >= service_length)
            return EAI_OVERFLOW;
    }
    return 0;
}
