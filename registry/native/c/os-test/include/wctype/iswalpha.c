#include <wctype.h>
#ifdef iswalpha
#undef iswalpha
#endif
int (*foo)(wint_t) = iswalpha;
int main(void) { return 0; }
