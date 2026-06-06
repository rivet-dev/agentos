#include <arpa/inet.h>
#ifndef ntohl
uint32_t (*foo)(uint32_t) = ntohl;
#endif
int main(void) { return 0; }
