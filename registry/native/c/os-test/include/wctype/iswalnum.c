#include <wctype.h>
#ifdef iswalnum
#undef iswalnum
#endif
int (*foo)(wint_t) = iswalnum;
int main(void) { return 0; }
