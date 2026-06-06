#include <endian.h>
#ifndef htole64
uint64_t (*foo)(uint64_t) = htole64;
#endif
int main(void) { return 0; }
