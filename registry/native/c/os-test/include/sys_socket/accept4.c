#include <sys/socket.h>
#ifdef accept4
#undef accept4
#endif
int (*foo)(int, struct sockaddr *restrict, socklen_t *restrict, int) = accept4;
int main(void) { return 0; }
