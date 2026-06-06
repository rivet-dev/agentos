#include <endian.h>
#ifndef htobe16
uint16_t (*foo)(uint16_t) = htobe16;
#endif
int main(void) { return 0; }
