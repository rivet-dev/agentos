#include <endian.h>
#ifndef htobe64
uint64_t (*foo)(uint64_t) = htobe64;
#endif
int main(void) { return 0; }
