#include <arpa/inet.h>
#ifdef inet_ntop
#undef inet_ntop
#endif
const char *(*foo)(int, const void *restrict, char *restrict, socklen_t) = inet_ntop;
int main(void) { return 0; }
