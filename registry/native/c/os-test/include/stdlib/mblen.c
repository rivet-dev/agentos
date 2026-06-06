#include <stdlib.h>
#ifdef mblen
#undef mblen
#endif
int (*foo)(const char *, size_t) = mblen;
int main(void) { return 0; }
