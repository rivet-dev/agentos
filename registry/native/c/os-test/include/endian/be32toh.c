#include <endian.h>
#ifndef be32toh
uint32_t (*foo)(uint32_t) = be32toh;
#endif
int main(void) { return 0; }
