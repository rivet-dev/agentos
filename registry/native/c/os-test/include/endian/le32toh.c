#include <endian.h>
#ifndef le32toh
uint32_t (*foo)(uint32_t) = le32toh;
#endif
int main(void) { return 0; }
