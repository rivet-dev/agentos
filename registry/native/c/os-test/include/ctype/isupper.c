#include <ctype.h>
#ifdef isupper
#undef isupper
#endif
int (*foo)(int) = isupper;
int main(void) { return 0; }
