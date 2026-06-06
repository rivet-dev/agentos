#include <ctype.h>
#ifdef isxdigit
#undef isxdigit
#endif
int (*foo)(int) = isxdigit;
int main(void) { return 0; }
