#include <netdb.h>
#ifdef getaddrinfo
#undef getaddrinfo
#endif
int (*foo)(const char *restrict, const char *restrict, const struct addrinfo *restrict, struct addrinfo **restrict) = getaddrinfo;
int main(void) { return 0; }
