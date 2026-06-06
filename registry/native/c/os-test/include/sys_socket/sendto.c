#include <sys/socket.h>
#ifdef sendto
#undef sendto
#endif
ssize_t (*foo)(int, const void *, size_t, int, const struct sockaddr *, socklen_t) = sendto;
int main(void) { return 0; }
