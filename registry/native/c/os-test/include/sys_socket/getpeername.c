#include <sys/socket.h>
#ifdef getpeername
#undef getpeername
#endif
int (*foo)(int, struct sockaddr *restrict, socklen_t *restrict) = getpeername;
int main(void) { return 0; }
