#include <sys/socket.h>
#ifdef connect
#undef connect
#endif
int (*foo)(int, const struct sockaddr *, socklen_t) = connect;
int main(void) { return 0; }
