#include <sys/socket.h>
#ifdef bind
#undef bind
#endif
int (*foo)(int, const struct sockaddr *, socklen_t) = bind;
int main(void) { return 0; }
