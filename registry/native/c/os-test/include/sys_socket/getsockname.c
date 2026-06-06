#include <sys/socket.h>
#ifdef getsockname
#undef getsockname
#endif
int (*foo)(int, struct sockaddr *restrict, socklen_t *restrict) = getsockname;
int main(void) { return 0; }
