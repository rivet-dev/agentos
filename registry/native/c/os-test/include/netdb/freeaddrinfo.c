#include <netdb.h>
#ifdef freeaddrinfo
#undef freeaddrinfo
#endif
void (*foo)(struct addrinfo *) = freeaddrinfo;
int main(void) { return 0; }
