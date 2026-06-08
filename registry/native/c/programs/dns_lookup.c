/* dns_lookup.c — resolve hostnames via getaddrinfo, print IP addresses */
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netdb.h>
#include <arpa/inet.h>

static int print_lookup(const char *label, const char *host, int family) {
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = family;
    hints.ai_socktype = SOCK_STREAM;

    struct addrinfo *res = NULL;
    int err = getaddrinfo(host, NULL, &hints, &res);
    if (err != 0) {
        fprintf(stderr, "%s getaddrinfo: %s\n", label, gai_strerror(err));
        return 1;
    }

    char ip[INET6_ADDRSTRLEN];
    const char *family_name = "unknown";
    if (res->ai_family == AF_INET) {
        struct sockaddr_in *sin = (struct sockaddr_in *)res->ai_addr;
        inet_ntop(AF_INET, &sin->sin_addr, ip, sizeof(ip));
        family_name = "AF_INET";
    } else if (res->ai_family == AF_INET6) {
        struct sockaddr_in6 *sin6 = (struct sockaddr_in6 *)res->ai_addr;
        inet_ntop(AF_INET6, &sin6->sin6_addr, ip, sizeof(ip));
        family_name = "AF_INET6";
    } else {
        snprintf(ip, sizeof(ip), "unsupported");
    }

    printf("%s host: %s\n", label, host);
    printf("%s family: %s\n", label, family_name);
    printf("%s ip: %s\n", label, ip);

    freeaddrinfo(res);
    return 0;
}

int main(int argc, char *argv[]) {
    const char *host = "localhost";
    if (argc >= 2)
        host = argv[1];

    if (print_lookup("inet4", host, AF_INET) != 0)
        return 1;
    if (print_lookup("inet6", "::1", AF_INET6) != 0)
        return 1;
    if (print_lookup("unspec", "127.0.0.1", AF_UNSPEC) != 0)
        return 1;

    return 0;
}
