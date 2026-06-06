#include <sys/socket.h>
#ifdef getsockopt
#undef getsockopt
#endif
int (*foo)(int, int, int, void *restrict, socklen_t *restrict) = getsockopt;
int main(void) { return 0; }
