#include <endian.h>
#ifndef htole16
uint16_t (*foo)(uint16_t) = htole16;
#endif
int main(void) { return 0; }
