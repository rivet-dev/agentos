#include <endian.h>
#ifndef le16toh
uint16_t (*foo)(uint16_t) = le16toh;
#endif
int main(void) { return 0; }
