#include <wchar.h>
#ifdef wcstold
#undef wcstold
#endif
long double (*foo)(const wchar_t *restrict, wchar_t **restrict) = wcstold;
int main(void) { return 0; }
