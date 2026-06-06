#include <endian.h>
#ifndef htobe32
uint32_t (*foo)(uint32_t) = htobe32;
#endif
int main(void) { return 0; }
