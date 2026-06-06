#include <netdb.h>
#ifdef getnameinfo
#undef getnameinfo
#endif
int (*foo)(const struct sockaddr *restrict, socklen_t, char *restrict, socklen_t, char *restrict, socklen_t, int) = getnameinfo;
int main(void) { return 0; }
