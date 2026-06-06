#include <endian.h>
#ifndef be16toh
uint16_t (*foo)(uint16_t) = be16toh;
#endif
int main(void) { return 0; }
