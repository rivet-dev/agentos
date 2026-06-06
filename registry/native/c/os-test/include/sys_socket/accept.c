#include <sys/socket.h>
#ifdef accept
#undef accept
#endif
int (*foo)(int, struct sockaddr *restrict, socklen_t *restrict) = accept;
int main(void) { return 0; }
