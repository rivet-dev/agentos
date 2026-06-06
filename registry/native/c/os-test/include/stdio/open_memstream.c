#include <stdio.h>
#ifdef open_memstream
#undef open_memstream
#endif
FILE *(*foo)(char **, size_t *) = open_memstream;
int main(void) { return 0; }
