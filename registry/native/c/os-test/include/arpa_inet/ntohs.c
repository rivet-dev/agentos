#include <arpa/inet.h>
#ifndef ntohs
uint16_t (*foo)(uint16_t) = ntohs;
#endif
int main(void) { return 0; }
