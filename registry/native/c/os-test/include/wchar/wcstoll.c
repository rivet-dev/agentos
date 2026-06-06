#include <wchar.h>
#ifdef wcstoll
#undef wcstoll
#endif
long long (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstoll;
int main(void) { return 0; }
