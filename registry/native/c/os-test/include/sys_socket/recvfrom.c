#include <sys/socket.h>
#ifdef recvfrom
#undef recvfrom
#endif
ssize_t (*foo)(int, void *restrict, size_t, int, struct sockaddr *restrict, socklen_t *restrict) = recvfrom;
int main(void) { return 0; }
