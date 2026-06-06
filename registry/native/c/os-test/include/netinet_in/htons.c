#include <netinet/in.h>
#ifndef htons
uint16_t (*foo)(uint16_t) = htons;
#endif
int main(void) { return 0; }
