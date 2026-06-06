#include <ctype.h>
#ifdef isalpha
#undef isalpha
#endif
int (*foo)(int) = isalpha;
int main(void) { return 0; }
