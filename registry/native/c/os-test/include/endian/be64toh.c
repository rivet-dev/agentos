#include <endian.h>
#ifndef be64toh
uint64_t (*foo)(uint64_t) = be64toh;
#endif
int main(void) { return 0; }
