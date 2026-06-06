#include <sys/socket.h>
#ifdef setsockopt
#undef setsockopt
#endif
int (*foo)(int, int, int, const void *, socklen_t) = setsockopt;
int main(void) { return 0; }
