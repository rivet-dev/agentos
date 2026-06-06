#include <endian.h>
#ifndef htole32
uint32_t (*foo)(uint32_t) = htole32;
#endif
int main(void) { return 0; }
