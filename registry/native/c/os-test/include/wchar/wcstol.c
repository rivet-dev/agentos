#include <wchar.h>
#ifdef wcstol
#undef wcstol
#endif
long (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstol;
int main(void) { return 0; }
