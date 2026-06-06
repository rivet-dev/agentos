#include <netinet/in.h>
#ifndef htonl
uint32_t (*foo)(uint32_t) = htonl;
#endif
int main(void) { return 0; }
