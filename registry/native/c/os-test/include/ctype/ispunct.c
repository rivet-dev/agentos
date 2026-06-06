#include <ctype.h>
#ifdef ispunct
#undef ispunct
#endif
int (*foo)(int) = ispunct;
int main(void) { return 0; }
