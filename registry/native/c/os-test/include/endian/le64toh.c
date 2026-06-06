#include <endian.h>
#ifndef le64toh
uint64_t (*foo)(uint64_t) = le64toh;
#endif
int main(void) { return 0; }
